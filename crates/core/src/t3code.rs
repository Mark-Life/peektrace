//! t3code thread names for the Claude and Codex sessions t3code started,
//! and the project of every session that ran in a linked worktree.
//!
//! t3code keeps its threads in a `SQLite` database under its state dir:
//! `statev2.sqlite` today, `state.sqlite` before. The newest file wins. It is
//! opened read-only and queried once per change; any error leaves sessions
//! as they are.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, ErrorCode, OpenFlags};
use tracing::{debug, info};

use crate::js::text;
use crate::model::{Agent, Launcher, SessionHeader, Worktree};
use crate::paths::PathStyle;
use crate::sessions::inputs::{Stamp, stamp};
use crate::sessions::model::ParsedSession;
use crate::worktree::{self, Known};

mod sql;

/// Database files, newest schema first.
const DB_NAMES: [&str; 2] = ["statev2.sqlite", "state.sqlite"];

/// The title t3code gives a thread until it names it.
const PLACEHOLDER_TITLE: &str = "New thread";

/// How long a read waits on t3code's write lock before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(500);

/// One t3code thread as the session it started sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// `None` when the thread is deleted, blank or not named yet: the
    /// session keeps its own title.
    pub title: Option<String>,
    pub launcher: Launcher,
}

/// Threads by the agent and native session id they ran, and what t3code
/// recorded for each worktree path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Index {
    threads: HashMap<(Agent, String), Thread>,
    worktrees: HashMap<String, Known>,
}

impl Index {
    fn get(&self, agent: Agent, id: &str) -> Option<&Thread> {
        self.threads.get(&(agent, id.to_owned()))
    }

    fn worktree(&self, path: &str) -> Option<Known> {
        self.worktrees.get(path).cloned()
    }
}

/// t3code's provider name for an agent peektrace reads.
fn agent_of(provider: &str) -> Option<Agent> {
    match provider {
        "claudeAgent" => Some(Agent::Claude),
        "codex" => Some(Agent::Codex),
        _ => None,
    }
}

/// Column text as transcript text; blank is `None`.
fn text_of(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
        .map(|s| text::escape_reserved(&s).into_owned())
}

fn run(conn: &Connection, sql: &str, index: &mut Index) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let provider: Option<String> = row.get(0)?;
        let id: Option<String> = row.get(1)?;
        let id = id.filter(|id| !id.is_empty());
        let (Some(agent), Some(id)) = (provider.as_deref().and_then(agent_of), id) else {
            continue;
        };
        let deleted: bool = row.get(5)?;
        let thread = Thread {
            title: text_of(row.get(2)?).filter(|t| !deleted && t != PLACEHOLDER_TITLE),
            launcher: Launcher {
                name: "t3code".to_owned(),
                branch: text_of(row.get(3)?),
                worktree_path: text_of(row.get(4)?),
                parent_title: text_of(row.get(6)?).filter(|t| t != PLACEHOLDER_TITLE),
            },
        };
        index.threads.insert((agent, id), thread);
    }
    Ok(())
}

fn run_worktrees(conn: &Connection, sql: &str, index: &mut Index) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let (Some(path), Some(project)) = (text_of(row.get(0)?), text_of(row.get(1)?)) else {
            continue;
        };
        let path = worktree::normalize(&path).to_owned();
        let root: Option<String> = text_of(row.get(2)?);
        // A dir inside the project (t3code's scratch dirs) is no worktree.
        if root
            .as_deref()
            .map(worktree::normalize)
            .is_some_and(|r| path == r || path.starts_with(&format!("{r}/")))
        {
            continue;
        }
        let known = Known {
            project,
            project_root: root,
            branch: text_of(row.get(3)?),
        };
        index.worktrees.insert(path, known);
    }
    Ok(())
}

/// Whether an error means t3code holds the lock, so a later read may work.
fn busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// Every thread in the database at `path`. `None` when it is locked or
/// cannot be opened; a query the schema lacks is skipped.
fn read_index(path: &str) -> Option<Index> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let opened = Connection::open_with_flags(path, flags).and_then(|conn| {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "query_only", true)?;
        Ok(conn)
    });
    let conn = match opened {
        Ok(conn) => conn,
        Err(e) => {
            debug!(db = %path, reason = %e, "t3code db skipped");
            return None;
        }
    };
    let mut index = Index::default();
    let results = [
        run(&conn, &sql::legacy_current(), &mut index)
            .or_else(|_| run(&conn, sql::LEGACY, &mut index)),
        run(&conn, &sql::current(), &mut index),
        run_worktrees(&conn, sql::LEGACY_WORKTREES, &mut index),
        run_worktrees(&conn, sql::WORKTREES, &mut index),
    ];
    for e in results.iter().filter_map(|r| r.as_ref().err()) {
        debug!(db = %path, reason = %e, "t3code query skipped");
        if busy(e) {
            return None;
        }
    }
    Some(index)
}

/// The newer stamp of a database file and its write-ahead log.
fn newest(path: &str) -> Option<SystemTime> {
    [stamp(path), stamp(&format!("{path}-wal"))]
        .into_iter()
        .flatten()
        .filter_map(|(_, m)| m)
        .max()
}

/// The database t3code writes now: of the files in `dir`, the one changed
/// last (its log counted), the newer schema on a tie.
fn current_db(dir: &str) -> Option<String> {
    DB_NAMES
        .iter()
        .map(|name| PathStyle::NATIVE.join(&[dir, name]))
        .filter_map(|path| newest(&path).map(|m| (m, path)))
        .reduce(|best, next| if next.0 > best.0 { next } else { best })
        .map(|(_, path)| path)
}

/// The stamps of a database file and its log when its index was read.
type Stamps = (Stamp, Stamp);

/// The last good index per database path.
static CACHE: Mutex<BTreeMap<String, (Stamps, Arc<Index>)>> = Mutex::new(BTreeMap::new());

/// The index for t3code's state dir, read again only when the database or
/// its log changed since the last call. While t3code holds the lock, the
/// last good index stands.
fn load(dir: Option<&str>) -> Arc<Index> {
    let Some(path) = dir.and_then(current_db) else {
        return Arc::default();
    };
    let stamps = (stamp(&path), stamp(&format!("{path}-wal")));
    let cached = CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&path)
        .cloned();
    if let Some((s, index)) = &cached
        && *s == stamps
    {
        return Arc::clone(index);
    }
    // Read outside the lock: a busy database must not stall other callers.
    let Some(index) = read_index(&path).map(Arc::new) else {
        return cached.map(|(_, index)| index).unwrap_or_default();
    };
    if cached.is_none() {
        info!(db = %path, threads = index.threads.len(), "t3code db read");
    }
    CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path, (stamps, Arc::clone(&index)));
    index
}

/// Agents t3code runs.
const fn t3code_runs(agent: Agent) -> bool {
    matches!(agent, Agent::Claude | Agent::Codex)
}

/// t3code's base dir (`~/.t3`): the parent of its state dir.
fn t3_home(dir: Option<&str>) -> Option<&str> {
    let dir = worktree::normalize(dir?);
    let cut = dir.rfind(['/', '\\'])?;
    (cut > 0).then(|| &dir[..cut])
}

/// Whether `cwd` is `root` or a dir below it.
fn within(cwd: &str, root: &str) -> bool {
    let (cwd, root) = (worktree::normalize(cwd), worktree::normalize(root));
    cwd.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}

/// The worktree a session in `cwd` ran in: by its cwd, else by the
/// worktree t3code gave its thread when the session has no cwd or ran
/// inside that worktree.
fn worktree_of(
    index: &Index,
    cwd: Option<&str>,
    launcher: Option<&Launcher>,
    dir: Option<&str>,
) -> Option<Worktree> {
    let home = t3_home(dir);
    let known = |p: &str| index.worktree(p);
    cwd.and_then(|c| worktree::resolve(c, known, home))
        .or_else(|| {
            let path = launcher?.worktree_path.as_deref()?;
            cwd.is_none_or(|c| within(c, path))
                .then(|| worktree::resolve(path, known, home))?
        })
}

/// Name and tag every header t3code started, and label each one that ran
/// in a linked worktree with its project.
pub fn apply_headers(headers: &mut [SessionHeader], dir: Option<&str>) {
    if headers.is_empty() {
        return;
    }
    let index = load(dir);
    for h in headers.iter_mut() {
        if let Some(t) = t3code_runs(h.agent)
            .then(|| index.get(h.agent, h.id.as_str()))
            .flatten()
        {
            if let Some(title) = &t.title {
                h.title = Some(title.clone());
            }
            h.launcher = Some(t.launcher.clone());
        }
        h.worktree = worktree_of(&index, h.cwd.as_deref(), h.launcher.as_ref(), dir);
    }
}

fn lookup(index: &Index, agent: Agent, id: &str) -> Option<Thread> {
    if !t3code_runs(agent) {
        return None;
    }
    index.get(agent, id).cloned()
}

/// The thread and worktree one parsed session was labelled from, so a
/// cached analysis can tell when t3code renamed or deleted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    dir: Option<String>,
    agent: Agent,
    id: String,
    cwd: Option<String>,
    seen: Option<Thread>,
    worktree: Option<Worktree>,
}

impl Mark {
    /// Whether t3code still holds the thread this mark saw, and the
    /// session's worktree still resolves the same.
    pub fn unchanged(&self) -> bool {
        let dir = self.dir.as_deref();
        let index = load(dir);
        let seen = lookup(&index, self.agent, &self.id);
        let launcher = seen.as_ref().map(|t| &t.launcher);
        seen == self.seen
            && worktree_of(&index, self.cwd.as_deref(), launcher, dir) == self.worktree
    }
}

/// Name and tag one parsed session t3code started, and label its worktree;
/// the mark records what was applied.
pub fn apply_parsed(agent: Agent, parsed: &mut ParsedSession, dir: Option<&str>) -> Mark {
    let index = load(dir);
    let seen = lookup(&index, agent, &parsed.session_id);
    if let Some(t) = &seen {
        if let Some(title) = &t.title {
            parsed.title = Some(title.clone());
        }
        parsed.launcher = Some(t.launcher.clone());
    }
    parsed.worktree = worktree_of(&index, parsed.cwd.as_deref(), parsed.launcher.as_ref(), dir);
    Mark {
        dir: dir.map(str::to_owned),
        agent,
        id: parsed.session_id.clone(),
        cwd: parsed.cwd.clone(),
        seen,
        worktree: parsed.worktree.clone(),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod worktree_tests;
