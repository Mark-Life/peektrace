//! File watcher over every agent root: classify each changed path, batch
//! the hits in a short window, and bump the `sessions` / `memory` versions
//! clients use to know when to refetch.

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use notify::event::CreateKind;
use notify::{Event, EventKind, RecursiveMode, Watcher as _};
use tracing::{error, info_span};

use crate::listing::AgentRoots;
use crate::model::Agent;
use crate::sessions::resolve;

/// A batch closes this long after its first event.
const WINDOW: Duration = Duration::from_millis(150);
/// A batch also closes once it holds this many events.
const MAX_BATCH: usize = 256;

/// Monotonic change counters; both start at 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchVersions {
    pub memory: u64,
    pub sessions: u64,
}

/// What a changed path invalidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Memory,
    Sessions,
}

fn segments(rel: &Path) -> Vec<&str> {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect()
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "file names match case-sensitively, as in TS"
)]
fn is_opencode_store(name: &str) -> bool {
    name.starts_with("opencode") && (name.ends_with(".db") || name.ends_with(".db-wal"))
}

/// Under an `opencode` `storage/{session,message,part}` dir.
fn in_store(segs: &[&str]) -> bool {
    segs.windows(2)
        .any(|w| w[0] == "storage" && matches!(w[1], "session" | "message" | "part"))
}

/// The scope a path relative to an `agent` root invalidates, if any.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "file names match case-sensitively, as in TS"
)]
pub fn classify(agent: Agent, rel: &Path) -> Option<Scope> {
    let segs = segments(rel);
    let name = *segs.last()?;
    match agent {
        Agent::Claude if name.ends_with(".md") && segs.contains(&"memory") => Some(Scope::Memory),
        Agent::Claude | Agent::Codex | Agent::Pi => {
            name.ends_with(".jsonl").then_some(Scope::Sessions)
        }
        Agent::Opencode => {
            let stored = name.ends_with(".json") && in_store(&segs);
            (stored || is_opencode_store(name)).then_some(Scope::Sessions)
        }
    }
}

/// The scope files in a new directory relative to an `agent` root can
/// invalidate. inotify watches a new directory only after its create event,
/// so files written into it right away raise no event of their own.
pub fn classify_dir(agent: Agent, rel: &Path) -> Option<Scope> {
    let segs = segments(rel);
    segs.last()?;
    match agent {
        Agent::Claude if segs.contains(&"memory") => Some(Scope::Memory),
        Agent::Claude | Agent::Codex | Agent::Pi => Some(Scope::Sessions),
        Agent::Opencode => in_store(&segs).then_some(Scope::Sessions),
    }
}

/// One watched root: the agent whose rules apply, as configured and as the
/// OS reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRoot {
    pub agent: Agent,
    pub path: PathBuf,
    pub real: PathBuf,
}

/// Every agent root that exists now, each path once per agent.
pub fn watch_roots(roots: &AgentRoots) -> Vec<WatchRoot> {
    let mut out: Vec<WatchRoot> = Vec::new();
    for agent in Agent::ALL {
        for s in roots.of(agent) {
            let root = s.projects_root.as_str();
            if root.is_empty() || !resolve::exists(root) {
                continue;
            }
            let path = PathBuf::from(root);
            if out.iter().any(|r| r.agent == agent && r.path == path) {
                continue;
            }
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            out.push(WatchRoot { agent, path, real });
        }
    }
    out
}

fn classify_under(
    roots: &[WatchRoot],
    path: &Path,
    rule: fn(Agent, &Path) -> Option<Scope>,
) -> Vec<Scope> {
    roots
        .iter()
        .filter_map(|r| {
            let rel = path
                .strip_prefix(&r.real)
                .or_else(|_| path.strip_prefix(&r.path))
                .ok()?;
            rule(r.agent, rel)
        })
        .collect()
}

/// Scopes one event path raises under every root that holds it.
pub fn classify_path(roots: &[WatchRoot], path: &Path) -> Vec<Scope> {
    classify_under(roots, path, classify)
}

/// Scopes one OS event raises. A rescan means events were lost, so it
/// raises every scope; a new directory raises what its files could.
pub fn event_scopes(
    roots: &[WatchRoot],
    event: &Event,
    is_dir: impl Fn(&Path) -> bool,
) -> Vec<Scope> {
    if event.need_rescan() {
        return vec![Scope::Sessions, Scope::Memory];
    }
    let rule = |p: &Path| match event.kind {
        EventKind::Create(CreateKind::Folder) => classify_dir,
        EventKind::Create(CreateKind::Any | CreateKind::Other) if is_dir(p) => classify_dir,
        _ => classify,
    };
    match event.kind {
        EventKind::Access(_) => Vec::new(),
        _ => event
            .paths
            .iter()
            .flat_map(|p| classify_under(roots, p, rule(p)))
            .collect(),
    }
}

/// Versions after one flushed batch.
pub fn bump(v: WatchVersions, batch: &[Scope]) -> WatchVersions {
    let saw = |s| u64::from(batch.contains(&s));
    WatchVersions {
        memory: v.memory + saw(Scope::Memory),
        sessions: v.sessions + saw(Scope::Sessions),
    }
}

/// Collect one batch: block for the first event, then take events until the
/// window closes or the batch is full. `None` once the sender is gone.
fn next_batch(rx: &Receiver<Scope>) -> Option<Vec<Scope>> {
    let first = rx.recv().ok()?;
    let deadline = Instant::now() + WINDOW;
    let mut batch = vec![first];
    while batch.len() < MAX_BATCH {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(s) => batch.push(s),
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => return Some(batch),
        }
    }
    Some(batch)
}

/// Why some changes will not reach clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchProblem {
    /// The OS watcher did not start, so nothing is watched.
    Unavailable { reason: String },
    /// The OS limit on watches was reached under this root, so part of it
    /// is not watched.
    Limit { path: PathBuf },
    /// This root is not watched.
    Root { path: PathBuf, reason: String },
}

impl WatchProblem {
    /// What the user misses and how to fix it, one sentence.
    pub fn message(&self) -> String {
        match self {
            Self::Limit { path } => format!(
                "live refresh misses changes under {}: the OS file-watch limit is reached. On Linux raise fs.inotify.max_user_watches (e.g. sudo sysctl fs.inotify.max_user_watches=524288) and restart.",
                path.display()
            ),
            Self::Root { path, reason } => {
                format!("live refresh is off for {} ({reason}).", path.display())
            }
            Self::Unavailable { reason } => format!("live refresh is off ({reason})."),
        }
    }
}

/// The problem a failed watch of `path` leaves.
pub fn root_problem(path: &Path, e: &notify::Error) -> WatchProblem {
    match e.kind {
        notify::ErrorKind::MaxFilesWatch => WatchProblem::Limit {
            path: path.to_path_buf(),
        },
        _ => WatchProblem::Root {
            path: path.to_path_buf(),
            reason: e.to_string(),
        },
    }
}

/// A running watcher. Dropping it stops the OS watch and the batch thread.
pub struct Watcher {
    _os: Option<notify::RecommendedWatcher>,
    versions: Arc<Mutex<WatchVersions>>,
    problems: Vec<WatchProblem>,
}

impl Watcher {
    /// Watch every existing root of `roots`; `on_flush` gets the versions
    /// after each batch that changed them. A root that is missing now is
    /// never watched.
    pub fn start(roots: &AgentRoots, on_flush: impl Fn(WatchVersions) + Send + 'static) -> Self {
        let versions = Arc::new(Mutex::new(WatchVersions::default()));
        let watched = watch_roots(roots);
        if watched.is_empty() {
            return Self {
                _os: None,
                versions,
                problems: Vec::new(),
            };
        }
        let (tx, rx) = mpsc::channel::<Scope>();
        let classify_roots = watched.clone();
        let handler = move |res: notify::Result<Event>| match res {
            Ok(event) => {
                for s in event_scopes(&classify_roots, &event, Path::is_dir) {
                    let _ = tx.send(s);
                }
            }
            Err(e) => error!(error = %e, "WatchService.pump failed"),
        };
        let mut problems = Vec::new();
        let os = match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                for r in &watched {
                    if let Err(e) = w.watch(&r.path, RecursiveMode::Recursive) {
                        error!(path = %r.path.display(), error = %e, "WatchService.pump failed");
                        problems.push(root_problem(&r.path, &e));
                    }
                }
                Some(w)
            }
            Err(e) => {
                error!(error = %e, "WatchService.pump failed");
                problems.push(WatchProblem::Unavailable {
                    reason: e.to_string(),
                });
                None
            }
        };
        let shared = Arc::clone(&versions);
        std::thread::spawn(move || {
            while let Some(batch) = next_batch(&rx) {
                let _span = info_span!("WatchService.flush", batch = batch.len()).entered();
                let next = {
                    let mut v = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    *v = bump(*v, &batch);
                    *v
                };
                on_flush(next);
            }
        });
        Self {
            _os: os,
            versions,
            problems,
        }
    }

    /// What failed when the watch started; empty when every root is watched.
    pub fn problems(&self) -> &[WatchProblem] {
        &self.problems
    }

    /// The current versions.
    pub fn versions(&self) -> WatchVersions {
        *self.versions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(agent: Agent, rel: &str) -> Option<Scope> {
        classify(agent, Path::new(rel))
    }

    #[test]
    fn claude_rules() {
        assert_eq!(c(Agent::Claude, "-proj/readme.md"), None);
        assert_eq!(c(Agent::Claude, "-proj/memory/a.md"), Some(Scope::Memory));
        assert_eq!(
            c(Agent::Claude, "-proj/memory/b.jsonl"),
            Some(Scope::Sessions)
        );
        assert_eq!(c(Agent::Claude, "-proj/memory/c.txt"), None);
        assert_eq!(c(Agent::Claude, "memory.md"), None);
        assert_eq!(c(Agent::Claude, "memory/top.md"), Some(Scope::Memory));
        assert_eq!(
            c(Agent::Claude, "-new/sub/memory/z.md"),
            Some(Scope::Memory)
        );
        assert_eq!(c(Agent::Claude, "-p/s.jsonl"), Some(Scope::Sessions));
    }

    #[test]
    fn other_agents() {
        assert_eq!(c(Agent::Codex, "2026/01/r.jsonl"), Some(Scope::Sessions));
        assert_eq!(c(Agent::Codex, "memory/a.md"), None);
        assert_eq!(c(Agent::Pi, "--p--/s.jsonl"), Some(Scope::Sessions));
        assert_eq!(c(Agent::Opencode, "opencode.db"), Some(Scope::Sessions));
        assert_eq!(
            c(Agent::Opencode, "opencode-dev.db-wal"),
            Some(Scope::Sessions)
        );
        assert_eq!(c(Agent::Opencode, "opencode.db-shm"), None);
        assert_eq!(
            c(Agent::Opencode, "storage/part/m/p.json"),
            Some(Scope::Sessions)
        );
        assert_eq!(c(Agent::Opencode, "storage/project/x.json"), None);
        assert_eq!(c(Agent::Opencode, "log/x.log"), None);
    }

    #[test]
    fn new_dirs_raise_what_their_files_could() {
        let d = |agent, rel| classify_dir(agent, Path::new(rel));
        assert_eq!(d(Agent::Claude, "-p/memory"), Some(Scope::Memory));
        assert_eq!(d(Agent::Claude, "-p"), Some(Scope::Sessions));
        assert_eq!(d(Agent::Claude, ""), None);
        assert_eq!(d(Agent::Codex, "2026/01/02"), Some(Scope::Sessions));
        assert_eq!(d(Agent::Pi, "--p--"), Some(Scope::Sessions));
        assert_eq!(
            d(Agent::Opencode, "storage/message/ses_1"),
            Some(Scope::Sessions)
        );
        assert_eq!(d(Agent::Opencode, "log"), None);
    }

    fn root() -> Vec<WatchRoot> {
        vec![WatchRoot {
            agent: Agent::Claude,
            path: PathBuf::from("/h/.claude/projects"),
            real: PathBuf::from("/h/.claude/projects"),
        }]
    }

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn a_memory_dir_made_with_its_files_bumps_memory() {
        let made = event(
            EventKind::Create(CreateKind::Folder),
            "/h/.claude/projects/-p/memory",
        );
        assert_eq!(event_scopes(&root(), &made, |_| false), vec![Scope::Memory]);
        let unknown = event(
            EventKind::Create(CreateKind::Any),
            "/h/.claude/projects/-p/memory",
        );
        assert_eq!(
            event_scopes(&root(), &unknown, |_| true),
            vec![Scope::Memory]
        );
        assert_eq!(event_scopes(&root(), &unknown, |_| false), vec![]);
    }

    #[test]
    fn a_rescan_bumps_every_scope_and_access_none() {
        let rescan = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert_eq!(
            event_scopes(&root(), &rescan, |_| false),
            vec![Scope::Sessions, Scope::Memory]
        );
        let read = event(
            EventKind::Access(notify::event::AccessKind::Any),
            "/h/.claude/projects/-p/s.jsonl",
        );
        assert_eq!(event_scopes(&root(), &read, |_| false), vec![]);
        let wrote = event(
            EventKind::Modify(notify::event::ModifyKind::Any),
            "/h/.claude/projects/-p/s.jsonl",
        );
        assert_eq!(
            event_scopes(&root(), &wrote, |_| false),
            vec![Scope::Sessions]
        );
    }

    #[test]
    fn the_watch_limit_is_its_own_problem() {
        let path = Path::new("/r");
        assert_eq!(
            root_problem(path, &notify::Error::new(notify::ErrorKind::MaxFilesWatch)),
            WatchProblem::Limit {
                path: path.to_path_buf()
            }
        );
        assert!(matches!(
            root_problem(path, &notify::Error::path_not_found()),
            WatchProblem::Root { .. }
        ));
    }

    #[test]
    fn bump_counts_each_scope_once() {
        let v = bump(
            WatchVersions::default(),
            &[Scope::Sessions, Scope::Sessions, Scope::Memory],
        );
        assert_eq!(
            v,
            WatchVersions {
                memory: 1,
                sessions: 1
            }
        );
        assert_eq!(bump(v, &[Scope::Sessions]).sessions, 2);
    }
}
