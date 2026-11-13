//! What the TUI asks of core, and the in-process implementation. Each call
//! runs under a root `tui.<method>` span.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::time::LocalZone;
use peektrace_core::memory::read::{AllVaults, all_vaults};
use peektrace_core::model::SessionHeader;
use peektrace_core::sessions::model::AnalyzedSession;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::settings_doc::{
    SettingsDoc, SettingsUpdateError, StoredSettings, read_stored, write_stored,
};
use peektrace_core::stats::accumulate::DrillTable;
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::report::{DrillResult, StatsReport};
use peektrace_core::stats::service::{DrillPage, ReportRequest, StatsEnv, drill, refresh, report};
use peektrace_core::{Agent, AgentRoots, ListFilter, list_sessions_cached};
use tracing::{Span, field, info_span};

/// Hits per drill page; one page is one corpus pass.
pub const DRILL_PAGE: usize = 100;

/// One backend call.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Sessions,
    Analyze {
        id: String,
        redact: bool,
    },
    /// A fingerprint of the files under `dirs` (a session's subagents).
    Stamp {
        dirs: Vec<String>,
    },
    Vaults,
    SettingsGet,
    SettingsSave {
        doc: SettingsDoc,
        expected: Option<f64>,
    },
    Report {
        days: u32,
        agent: Option<Agent>,
    },
    Drill {
        days: u32,
        agent: Option<Agent>,
        table: DrillTable,
        key: String,
        offset: usize,
    },
    Refresh {
        agent: Option<Agent>,
    },
}

/// One request in flight per slot; a newer request supersedes an older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Sessions,
    Analyze,
    Stamp,
    Vaults,
    Settings,
    Save,
    Report,
    Drill,
    Refresh,
}

impl Request {
    pub const fn slot(&self) -> Slot {
        match self {
            Self::Sessions => Slot::Sessions,
            Self::Analyze { .. } => Slot::Analyze,
            Self::Stamp { .. } => Slot::Stamp,
            Self::Vaults => Slot::Vaults,
            Self::SettingsGet => Slot::Settings,
            Self::SettingsSave { .. } => Slot::Save,
            Self::Report { .. } => Slot::Report,
            Self::Drill { .. } => Slot::Drill,
            Self::Refresh { .. } => Slot::Refresh,
        }
    }
}

/// Why a settings save failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The file moved since it was read.
    Changed,
    Other(String),
}

#[derive(Debug, Clone)]
pub enum Response {
    Sessions(Result<Vec<SessionHeader>, String>),
    Analyze(Result<Box<AnalyzedSession>, String>),
    Stamp(Result<u64, String>),
    Vaults(Result<Box<AllVaults>, String>),
    Settings(Result<StoredSettings, String>),
    Saved(Result<StoredSettings, SaveError>),
    Report(Result<Box<StatsReport>, String>),
    Drill(Result<Box<DrillResult>, String>),
    Refreshed(Result<(), String>),
}

impl Response {
    /// The error response for `slot`.
    pub fn failed(slot: Slot, why: String) -> Self {
        match slot {
            Slot::Sessions => Self::Sessions(Err(why)),
            Slot::Analyze => Self::Analyze(Err(why)),
            Slot::Stamp => Self::Stamp(Err(why)),
            Slot::Vaults => Self::Vaults(Err(why)),
            Slot::Settings => Self::Settings(Err(why)),
            Slot::Save => Self::Saved(Err(SaveError::Other(why))),
            Slot::Report => Self::Report(Err(why)),
            Slot::Drill => Self::Drill(Err(why)),
            Slot::Refresh => Self::Refreshed(Err(why)),
        }
    }
}

/// A queued call: its id, slot and request.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: u64,
    pub req: Request,
}

/// A finished call.
#[derive(Debug)]
pub struct Reply {
    pub id: u64,
    pub slot: Slot,
    pub resp: Response,
}

/// Where the TUI's data comes from. Calls block; the caller runs them off
/// the draw loop.
pub trait Backend: Send + Sync {
    fn call(&self, req: &Request) -> Response;
}

/// Runs `f` while holding the lock every corpus-wide stats pass takes, so
/// the TUI and the web server never run two at once.
pub type StatsGate = Arc<dyn Fn(&mut dyn FnMut()) + Send + Sync>;

/// A gate local to this process.
pub fn local_gate() -> StatsGate {
    let lock = Arc::new(Mutex::new(()));
    Arc::new(move |f: &mut dyn FnMut()| {
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        f();
    })
}

/// The in-process backend over `peektrace-core`.
pub struct CoreBackend {
    pub roots: AgentRoots,
    pub settings_path: String,
    pub read_only: bool,
    pub zone: Arc<dyn LocalZone + Send + Sync>,
    pub headers: HeaderCache,
    pub stats_cache_root: String,
    pub stats_gate: StatsGate,
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

const READ_ONLY: &str = "read-only mode is enabled; refusing to write";

fn root_span(req: &Request) -> Span {
    macro_rules! root {
        ($name:literal) => {
            info_span!(
                parent: None,
                $name,
                peektrace.root = true,
                peektrace.kind = "tui",
                sessions.unreadable = field::Empty,
                error = field::Empty
            )
        };
    }
    match req {
        Request::Sessions => root!("tui.sessions.list"),
        Request::Analyze { .. } => root!("tui.sessions.analyze"),
        Request::Stamp { .. } => root!("tui.sessions.stamp"),
        Request::Vaults => root!("tui.memory.allVaults"),
        Request::SettingsGet => root!("tui.settings.get"),
        Request::SettingsSave { .. } => root!("tui.settings.update"),
        Request::Report { .. } => root!("tui.stats.report"),
        Request::Drill { .. } => root!("tui.stats.drill"),
        Request::Refresh { .. } => root!("tui.stats.refresh"),
    }
}

/// Path, size and mtime of every file under `dirs`, hashed; a missing or
/// unreadable dir counts as empty.
fn tree_stamp(dirs: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    let mut stack: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                e.path().hash(&mut h);
                meta.len().hash(&mut h);
                meta.modified().ok().hash(&mut h);
            }
        }
    }
    h.finish()
}

fn record<T, E: std::fmt::Display>(span: &Span, r: &Result<T, E>) {
    if let Err(e) = r {
        span.record("error", field::display(e));
    }
}

impl CoreBackend {
    fn stats<T>(&self, f: impl FnOnce(&StatsEnv<'_>) -> T) -> T {
        let mut f = Some(f);
        let mut out = None;
        (self.stats_gate)(&mut || {
            if let Some(f) = f.take() {
                let env = StatsEnv {
                    roots: &self.roots,
                    cache: StatsCache::new(self.stats_cache_root.clone()),
                    local: self.zone.as_ref(),
                    now_ms: (self.now)(),
                    progress: None,
                };
                out = Some(f(&env));
            }
        });
        out.unwrap_or_else(|| unreachable!("the stats gate runs its closure"))
    }

    fn scope(days: u32, agent: Option<Agent>) -> ReportRequest {
        ReportRequest {
            agents: agent.map(|a| vec![a]),
            days: Some(f64::from(days)),
            force: false,
            limit: None,
        }
    }
}

impl Backend for CoreBackend {
    fn call(&self, req: &Request) -> Response {
        let span = root_span(req);
        let _entered = span.enter();
        match req {
            Request::Sessions => {
                let listing =
                    list_sessions_cached(&self.roots, &ListFilter::default(), &self.headers);
                span.record("sessions.unreadable", listing.unreadable.len());
                Response::Sessions(Ok(listing.headers))
            }
            Request::Analyze { id, redact } => {
                let env = SessionEnv {
                    roots: &self.roots,
                    local: self.zone.as_ref(),
                };
                let r = analyze_session(env, Target::Lookup(id), None, *redact);
                record(&span, &r);
                Response::Analyze(r.map(Box::new).map_err(|e| e.to_string()))
            }
            Request::Stamp { dirs } => Response::Stamp(Ok(tree_stamp(dirs))),
            Request::Vaults => Response::Vaults(Ok(Box::new(all_vaults(&self.roots.claude)))),
            Request::SettingsGet => Response::Settings(Ok(read_stored(&self.settings_path))),
            Request::SettingsSave { doc, expected } => {
                let r = if self.read_only {
                    Err(SettingsUpdateError::Write {
                        path: "settings.json".to_owned(),
                        reason: READ_ONLY.to_owned(),
                    })
                } else {
                    write_stored(&self.settings_path, doc.clone(), *expected)
                };
                record(&span, &r);
                Response::Saved(r.map_err(|e| match e {
                    SettingsUpdateError::Changed { .. } => SaveError::Changed,
                    e @ SettingsUpdateError::Write { .. } => SaveError::Other(e.to_string()),
                }))
            }
            Request::Report { days, agent } => {
                let r = self.stats(|env| report(env, &Self::scope(*days, *agent)));
                record(&span, &r);
                Response::Report(r.map(Box::new).map_err(|e| e.to_string()))
            }
            Request::Drill {
                days,
                agent,
                table,
                key,
                offset,
            } => {
                #[expect(clippy::cast_precision_loss, reason = "page offsets are small")]
                let page = DrillPage {
                    offset: Some(*offset as f64),
                    page_size: Some(DRILL_PAGE as f64),
                };
                let r =
                    self.stats(|env| drill(env, &Self::scope(*days, *agent), *table, key, page));
                record(&span, &r);
                Response::Drill(r.map(Box::new).map_err(|e| e.to_string()))
            }
            Request::Refresh { agent } => {
                let agents = agent.map(|a| vec![a]);
                let r = self.stats(|env| refresh(env, agents.as_deref(), true).map(|_| ()));
                record(&span, &r);
                Response::Refreshed(r.map_err(|e| e.to_string()))
            }
        }
    }
}
