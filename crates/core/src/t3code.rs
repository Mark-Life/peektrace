//! t3code thread names for the Claude and Codex sessions t3code started.
//!
//! t3code keeps its threads in a `SQLite` database under its state dir:
//! `statev2.sqlite` today, `state.sqlite` before. The newest file wins. It is
//! opened read-only and queried once per change; any error leaves sessions
//! as they are.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OpenFlags};
use tracing::{debug, info};

use crate::js::text;
use crate::model::{Agent, Launcher, SessionHeader};
use crate::paths::PathStyle;
use crate::sessions::inputs::{Stamp, stamp};
use crate::sessions::model::ParsedSession;

/// Database files, newest schema first.
const DB_NAMES: [&str; 2] = ["statev2.sqlite", "state.sqlite"];

/// The title t3code gives a thread until it names it.
const PLACEHOLDER_TITLE: &str = "New thread";

/// How long a read waits on t3code's write lock before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(500);

// Every query yields `(provider, native id, title, branch, worktree, deleted)`.

/// Legacy runtime rows with their legacy thread rows, for `state.sqlite`.
const LEGACY: &str = "SELECT r.provider_name,
        CASE r.provider_name WHEN 'codex' THEN json_extract(r.resume_cursor_json, '$.threadId')
            ELSE json_extract(r.resume_cursor_json, '$.resume') END,
        t.title, t.branch, t.worktree_path, t.deleted_at IS NOT NULL
     FROM provider_session_runtime r JOIN projection_threads t ON t.thread_id = r.thread_id
     WHERE json_valid(r.resume_cursor_json)";

/// Legacy runtime rows named by the current thread rows, whose titles and
/// deletes stay fresh.
const LEGACY_CURRENT: &str = "SELECT r.provider_name,
        CASE r.provider_name WHEN 'codex' THEN json_extract(r.resume_cursor_json, '$.threadId')
            ELSE json_extract(r.resume_cursor_json, '$.resume') END,
        t.title, json_extract(t.payload_json, '$.branch'),
        json_extract(t.payload_json, '$.worktreePath'), t.deleted_at IS NOT NULL
     FROM provider_session_runtime r
     JOIN orchestration_v2_projection_threads t ON t.thread_id = r.thread_id
     WHERE json_valid(r.resume_cursor_json) AND json_valid(t.payload_json)";

/// Current provider threads; the newest row for a native id wins.
const CURRENT: &str = "SELECT json_extract(p.payload_json, '$.nativeThreadRef.driver'),
        json_extract(p.payload_json, '$.nativeThreadRef.nativeId'),
        t.title, json_extract(t.payload_json, '$.branch'),
        json_extract(t.payload_json, '$.worktreePath'), t.deleted_at IS NOT NULL
     FROM orchestration_v2_projection_provider_threads p
     JOIN orchestration_v2_projection_threads t ON t.thread_id = p.thread_id
     WHERE json_valid(p.payload_json) AND json_valid(t.payload_json)
     ORDER BY p.updated_at";

/// One t3code thread as the session it started sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// `None` when the thread is deleted, blank or not named yet: the
    /// session keeps its own title.
    pub title: Option<String>,
    pub launcher: Launcher,
}

/// Threads by the agent and native session id they ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Index(HashMap<(Agent, String), Thread>);

impl Index {
    fn get(&self, agent: Agent, id: &str) -> Option<&Thread> {
        self.0.get(&(agent, id.to_owned()))
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
            },
        };
        index.0.insert((agent, id), thread);
    }
    Ok(())
}

/// Every thread in the database at `path`; empty when it cannot be read.
fn read_index(path: &str) -> Index {
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
            return Index::default();
        }
    };
    let mut index = Index::default();
    let skipped = |e: rusqlite::Error| debug!(db = %path, reason = %e, "t3code query skipped");
    run(&conn, LEGACY_CURRENT, &mut index)
        .or_else(|_| run(&conn, LEGACY, &mut index))
        .unwrap_or_else(skipped);
    run(&conn, CURRENT, &mut index).unwrap_or_else(skipped);
    index
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

/// What the cached index was read from.
type Key = (String, Stamp, Stamp);

static CACHE: Mutex<Option<(Key, Arc<Index>)>> = Mutex::new(None);

/// The index for t3code's state dir, read again only when the database or
/// its log changed since the last call.
fn load(dir: Option<&str>) -> Arc<Index> {
    let Some(path) = dir.and_then(current_db) else {
        return Arc::default();
    };
    let key = (path.clone(), stamp(&path), stamp(&format!("{path}-wal")));
    let first = {
        let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        match cache.as_ref() {
            Some((k, index)) if *k == key => return Arc::clone(index),
            other => other.is_none_or(|(k, _)| k.0 != path),
        }
    };
    // Read outside the lock: a busy database must not stall other callers.
    let index = Arc::new(read_index(&path));
    if first {
        info!(db = %path, threads = index.0.len(), "t3code db read");
    }
    *CACHE.lock().unwrap_or_else(PoisonError::into_inner) = Some((key, Arc::clone(&index)));
    index
}

/// Agents t3code runs.
const fn t3code_runs(agent: Agent) -> bool {
    matches!(agent, Agent::Claude | Agent::Codex)
}

/// Name and tag every header t3code started.
pub fn apply_headers(headers: &mut [SessionHeader], dir: Option<&str>) {
    if !headers.iter().any(|h| t3code_runs(h.agent)) {
        return;
    }
    let index = load(dir);
    for h in headers.iter_mut() {
        if let Some(t) = index.get(h.agent, h.id.as_str()) {
            if let Some(title) = &t.title {
                h.title = Some(title.clone());
            }
            h.launcher = Some(t.launcher.clone());
        }
    }
}

fn lookup(dir: Option<&str>, agent: Agent, id: &str) -> Option<Thread> {
    if !t3code_runs(agent) {
        return None;
    }
    load(dir).get(agent, id).cloned()
}

/// The thread one parsed session was named from, so a cached analysis can
/// tell when t3code renamed or deleted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    dir: Option<String>,
    agent: Agent,
    id: String,
    seen: Option<Thread>,
}

impl Mark {
    /// Whether t3code still holds the thread this mark saw.
    pub fn unchanged(&self) -> bool {
        lookup(self.dir.as_deref(), self.agent, &self.id) == self.seen
    }
}

/// Name and tag one parsed session t3code started; the mark records what
/// was applied.
pub fn apply_parsed(agent: Agent, parsed: &mut ParsedSession, dir: Option<&str>) -> Mark {
    let seen = lookup(dir, agent, &parsed.session_id);
    if let Some(t) = &seen {
        if let Some(title) = &t.title {
            parsed.title = Some(title.clone());
        }
        parsed.launcher = Some(t.launcher.clone());
    }
    Mark {
        dir: dir.map(str::to_owned),
        agent,
        id: parsed.session_id.clone(),
        seen,
    }
}

#[cfg(test)]
mod tests;
