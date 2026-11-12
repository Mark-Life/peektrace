//! The native source: core runs on a worker thread per call, never on the
//! UI thread, under a root `app.<method>` span.

use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};
use peektrace_core::PathStyle;
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::time::LocalZone;
use peektrace_core::roots::{self, RootsEnv};
use peektrace_core::sessions::resolve::SessionError;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::sessions::write::{analyzed_json_with, listing_json};
use peektrace_core::watch::Watcher;
use peektrace_core::{AgentRoots, ListFilter, list_sessions_cached};
use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{ApiError, WatchVersions};
use tracing::{field, info_span};

use super::{DataSource, LoadError, OnVersions, Reply, decode};

/// The system zone (or `TZ`) for timestamps without an offset.
#[derive(Debug)]
pub struct SystemZone(TimeZone);

impl SystemZone {
    pub fn load() -> Self {
        Self(TimeZone::try_system().unwrap_or(TimeZone::UTC))
    }
}

impl LocalZone for SystemZone {
    fn offset_at_wall_ms(&self, wall_ms: i64) -> i64 {
        let (min, max) = (
            Timestamp::MIN.as_millisecond(),
            Timestamp::MAX.as_millisecond(),
        );
        let Ok(wall) = Timestamp::from_millisecond(wall_ms.clamp(min, max)) else {
            return 0;
        };
        self.0
            .to_ambiguous_timestamp(Offset::UTC.to_datetime(wall))
            .compatible()
            .map_or(0, |at| wall.as_millisecond() - at.as_millisecond())
    }
}

struct Inner {
    roots: AgentRoots,
    headers: HeaderCache,
    zone: SystemZone,
}

pub struct InProcess {
    inner: Arc<Inner>,
    watcher: Mutex<Option<Watcher>>,
}

macro_rules! root_span {
    ($name:literal) => {
        info_span!(
            parent: None,
            $name,
            peektrace.root = true,
            peektrace.kind = "app",
            sessions.unreadable = field::Empty,
            session.id = field::Empty,
            session.parent = field::Empty,
            redact = field::Empty,
            error = field::Empty
        )
    };
}

fn session_error(e: SessionError) -> LoadError {
    LoadError::Api(match e {
        SessionError::NotFound { id, searched } => ApiError::SessionNotFoundError {
            id,
            searched_root: searched,
        },
        SessionError::Read { path, reason } => ApiError::TranscriptParseError { path, reason },
    })
}

impl InProcess {
    pub fn new(roots: AgentRoots) -> Self {
        Self {
            inner: Arc::new(Inner {
                roots,
                headers: HeaderCache::default(),
                zone: SystemZone::load(),
            }),
            watcher: Mutex::new(None),
        }
    }

    /// Roots from `HOME`, the agent env vars and `settings.json`, as the CLI
    /// reads them.
    pub fn from_env() -> Self {
        let home = std::env::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::for_home(home, |k| {
            std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
        })
    }

    /// Roots under `home`, with env vars read through `var`.
    pub fn for_home(home: String, var: impl Fn(&str) -> Option<String>) -> Self {
        let env = RootsEnv::from_vars(home, PathStyle::NATIVE, var);
        let settings = std::fs::read(env.settings_path())
            .ok()
            .map(|b| roots::parse_settings(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
        Self::new(AgentRoots::resolve(&env, &settings))
    }

    fn list(inner: &Inner) -> Result<SessionListingView, LoadError> {
        let span = root_span!("app.sessions.list");
        let _entered = span.enter();
        let listing = list_sessions_cached(&inner.roots, &ListFilter::default(), &inner.headers);
        span.record("sessions.unreadable", listing.unreadable.len());
        let out = decode(&listing_json(&listing));
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
    }

    fn analyze(
        inner: &Inner,
        id: &str,
        parent: Option<&str>,
    ) -> Result<AnalyzedSession, LoadError> {
        let span = root_span!("app.sessions.analyze");
        span.record("session.id", id);
        if let Some(p) = parent {
            span.record("session.parent", p);
        }
        span.record("redact", true);
        let _entered = span.enter();
        let env = SessionEnv {
            roots: &inner.roots,
            local: &inner.zone,
        };
        let out = analyze_session(env, Target::Lookup(id), parent, true)
            .map_err(session_error)
            .and_then(|a| decode(&analyzed_json_with(&a, false)));
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
    }

    fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Inner) -> Result<T, LoadError> + Send + 'static,
        reply: Reply<T>,
    ) {
        let inner = Arc::clone(&self.inner);
        let spawned = std::thread::Builder::new()
            .name("peektrace-load".into())
            .spawn(move || reply(work(&inner)));
        if let Err(e) = spawned {
            tracing::error!(error = %e, "could not start a load thread");
        }
    }
}

impl DataSource for InProcess {
    fn sessions(&self, reply: Reply<SessionListingView>) {
        self.spawn(Self::list, reply);
    }

    fn analysis(&self, id: String, parent: Option<String>, reply: Reply<AnalyzedSession>) {
        self.spawn(
            move |inner| Self::analyze(inner, &id, parent.as_deref()),
            reply,
        );
    }

    fn watch(&self, on_versions: OnVersions) {
        let span = root_span!("app.watch.events");
        let on_versions: Arc<dyn Fn(WatchVersions) + Send + Sync> = Arc::from(on_versions);
        let notify = Arc::clone(&on_versions);
        let watcher = Watcher::start(&self.inner.roots, move |v| {
            notify(WatchVersions {
                memory: v.memory,
                sessions: v.sessions,
            });
        });
        if let Some(p) = watcher.problems().first() {
            span.record("error", field::debug(p));
        }
        let now = watcher.versions();
        on_versions(WatchVersions {
            memory: now.memory,
            sessions: now.sessions,
        });
        *self.watcher.lock().unwrap_or_else(PoisonError::into_inner) = Some(watcher);
    }

    fn sessions_now(&self) -> Option<Result<SessionListingView, LoadError>> {
        Some(Self::list(&self.inner))
    }

    fn analysis_now(
        &self,
        id: &str,
        parent: Option<&str>,
    ) -> Option<Result<AnalyzedSession, LoadError>> {
        Some(Self::analyze(&self.inner, id, parent))
    }
}
