//! File watcher over every agent root: classify each changed path, batch
//! the hits in a short window, and bump the `sessions` / `memory` versions
//! clients use to know when to refetch.

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher as _};
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

fn is_opencode_store(name: &str) -> bool {
    name.starts_with("opencode") && (name.ends_with(".db") || name.ends_with(".db-wal"))
}

/// The scope a path relative to an `agent` root invalidates, if any.
pub fn classify(agent: Agent, rel: &Path) -> Option<Scope> {
    let segs = segments(rel);
    let name = *segs.last()?;
    match agent {
        Agent::Claude if name.ends_with(".md") && segs.contains(&"memory") => Some(Scope::Memory),
        Agent::Claude | Agent::Codex | Agent::Pi => {
            name.ends_with(".jsonl").then_some(Scope::Sessions)
        }
        Agent::Opencode => {
            let stored = name.ends_with(".json")
                && segs
                    .windows(2)
                    .any(|w| w[0] == "storage" && matches!(w[1], "session" | "message" | "part"));
            (stored || is_opencode_store(name)).then_some(Scope::Sessions)
        }
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

/// Scopes one event path raises under every root that holds it.
pub fn classify_path(roots: &[WatchRoot], path: &Path) -> Vec<Scope> {
    roots
        .iter()
        .filter_map(|r| {
            let rel = path
                .strip_prefix(&r.real)
                .or_else(|_| path.strip_prefix(&r.path))
                .ok()?;
            classify(r.agent, rel)
        })
        .collect()
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

/// A running watcher. Dropping it stops the OS watch and the batch thread.
pub struct Watcher {
    _os: Option<notify::RecommendedWatcher>,
    versions: Arc<Mutex<WatchVersions>>,
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
            };
        }
        let (tx, rx) = mpsc::channel::<Scope>();
        let classify_roots = watched.clone();
        let handler = move |res: notify::Result<notify::Event>| match res {
            Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                for p in &event.paths {
                    for s in classify_path(&classify_roots, p) {
                        let _ = tx.send(s);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => error!(error = %e, "WatchService.pump failed"),
        };
        let os = match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                for r in &watched {
                    if let Err(e) = w.watch(&r.path, RecursiveMode::Recursive) {
                        error!(path = %r.path.display(), error = %e, "WatchService.pump failed");
                    }
                }
                Some(w)
            }
            Err(e) => {
                error!(error = %e, "WatchService.pump failed");
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
        Self { _os: os, versions }
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
