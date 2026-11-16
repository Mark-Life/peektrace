//! The native source: core runs on a worker thread per call, never on the
//! UI thread, under a root `app.<method>` span.

mod stats;

use std::sync::{Arc, Mutex, PoisonError};

use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::system_zone::SystemZone;
use peektrace_core::js::value::JsValue;
use peektrace_core::roots::{self, RootsEnv};
use peektrace_core::sessions::resolve::SessionError;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::sessions::write::{analyzed_json_with, listing_json};
use peektrace_core::settings_doc;
use peektrace_core::stats::cache::cache_root;
use peektrace_core::watch::{WatchProblem, Watcher};
use peektrace_core::{AgentRoots, ListFilter, PathStyle, list_sessions_cached, wire};
use peektrace_protocol::stats::DrillView;
use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{
    ApiError, Capability, ServerInfo, SettingsResult, SettingsUpdate, WatchVersions,
};
use tracing::{field, info_span};

use self::stats::{Drilled, Gate};
use super::memory::{self, MemoryData, MemoryDone, MemoryWrite};
use super::{DataSource, LoadError, OnVersions, Reply, StatsScreen, WatchEvent, decode};
use crate::model::stats::{DrillAsk, StatsScope};

struct Inner {
    roots: AgentRoots,
    tmp_dir: String,
    headers: HeaderCache,
    zone: SystemZone,
    stats_cache: String,
    /// One corpus pass at a time, so a second one reads the first one's cache.
    stats_lock: Mutex<()>,
    /// The scope of the last ask; a queued pass for another scope is skipped.
    wanted: Mutex<Option<StatsScope>>,
    /// Every hit of the last drilled row, so a page turn is a slice.
    drilled: Mutex<Option<Drilled>>,
    /// A fixed clock for headless runs and tests.
    now: Option<f64>,
    settings_path: String,
    user_home: String,
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct InProcess {
    inner: Arc<Inner>,
    read_only: bool,
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

fn bad_request(reason: impl Into<String>) -> LoadError {
    LoadError::Api(ApiError::BadRequestError {
        reason: reason.into(),
    })
}

impl InProcess {
    /// Sessions under `roots`; the stats fact cache in `stats_cache`.
    pub fn new(roots: AgentRoots, stats_cache: String) -> Self {
        Self::with_env(roots, stats_cache, String::new(), String::new())
    }

    fn with_env(
        roots: AgentRoots,
        stats_cache: String,
        settings_path: String,
        user_home: String,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                roots,
                tmp_dir: peektrace_core::js::os::tmpdir(),
                headers: HeaderCache::default(),
                zone: SystemZone::default(),
                stats_cache,
                stats_lock: Mutex::new(()),
                wanted: Mutex::new(None),
                drilled: Mutex::new(None),
                now: crate::fixed_now(),
                settings_path,
                user_home,
            }),
            read_only: false,
            watcher: Mutex::new(None),
        }
    }

    /// The same source with its stats clock fixed at `now_ms`.
    #[must_use]
    pub fn at(mut self, now_ms: f64) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.now = Some(now_ms);
        }
        self
    }

    /// Refuse settings writes, as a `--read-only` server does.
    #[must_use]
    pub const fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
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
        let settings_path = env.settings_path();
        let settings = std::fs::read(&settings_path)
            .ok()
            .map(|b| roots::parse_settings(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
        let roots = AgentRoots::resolve(&env, &settings);
        Self::with_env(roots, cache_root(&env), settings_path, env.home)
    }

    fn capabilities_list() -> Vec<Capability> {
        let _span = root_span!("app.capabilities.list").entered();
        wire::capability_rows()
    }

    fn info(inner: &Inner, read_only: bool) -> ServerInfo {
        let _span = root_span!("app.server.info").entered();
        wire::server_info(&inner.roots, read_only, &inner.user_home)
    }

    fn read_settings(inner: &Inner) -> Result<SettingsResult, LoadError> {
        let span = root_span!("app.settings.get");
        let _entered = span.enter();
        let out = decode(&wire::settings_body(&settings_doc::read_stored(
            &inner.settings_path,
        )));
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
    }

    fn write_settings(
        inner: &Inner,
        read_only: bool,
        update: &SettingsUpdate,
    ) -> Result<SettingsResult, LoadError> {
        let span = root_span!("app.settings.update");
        let _entered = span.enter();
        let out = (|| {
            wire::check_settings_writable(read_only).map_err(LoadError::Api)?;
            let text =
                serde_json::to_string(&update.settings).map_err(|e| bad_request(e.to_string()))?;
            let value = JsValue::parse(&text).ok_or_else(|| bad_request("body is not JSON"))?;
            let doc = wire::validated_settings(&value).map_err(LoadError::Api)?;
            let stored = wire::store_settings(&inner.settings_path, doc, update.expected_mtime)
                .map_err(LoadError::Api)?;
            decode(&wire::settings_body(&stored))
        })();
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
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

    fn capabilities(&self, reply: Reply<Vec<Capability>>) {
        self.spawn(|_| Ok(Self::capabilities_list()), reply);
    }

    fn server_info(&self, reply: Reply<ServerInfo>) {
        let read_only = self.read_only;
        self.spawn(move |inner| Ok(Self::info(inner, read_only)), reply);
    }

    fn settings(&self, reply: Reply<SettingsResult>) {
        self.spawn(Self::read_settings, reply);
    }

    fn save_settings(&self, update: SettingsUpdate, reply: Reply<SettingsResult>) {
        let read_only = self.read_only;
        self.spawn(
            move |inner| Self::write_settings(inner, read_only, &update),
            reply,
        );
    }

    fn watch(&self, on_versions: OnVersions) {
        let span = root_span!("app.watch.events");
        let on_versions: Arc<dyn Fn(WatchEvent) + Send + Sync> = Arc::from(on_versions);
        let notify = Arc::clone(&on_versions);
        let watcher = Watcher::start(&self.inner.roots, move |v| {
            notify(WatchEvent::Versions(WatchVersions {
                memory: v.memory,
                sessions: v.sessions,
            }));
        });
        let problems: Vec<String> = watcher
            .problems()
            .iter()
            .map(WatchProblem::message)
            .collect();
        if let Some(p) = problems.first() {
            span.record("error", p.as_str());
        }
        if !problems.is_empty() {
            for p in &problems {
                eprintln!("peektrace-app: warning: {p}");
            }
            on_versions(WatchEvent::Problems(problems));
        }
        let now = watcher.versions();
        on_versions(WatchEvent::Versions(WatchVersions {
            memory: now.memory,
            sessions: now.sessions,
        }));
        *locked(&self.watcher) = Some(watcher);
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

    fn capabilities_now(&self) -> Option<Result<Vec<Capability>, LoadError>> {
        Some(Ok(Self::capabilities_list()))
    }

    fn server_info_now(&self) -> Option<Result<ServerInfo, LoadError>> {
        Some(Ok(Self::info(&self.inner, self.read_only)))
    }

    fn settings_now(&self) -> Option<Result<SettingsResult, LoadError>> {
        Some(Self::read_settings(&self.inner))
    }

    fn save_settings_now(
        &self,
        update: SettingsUpdate,
    ) -> Option<Result<SettingsResult, LoadError>> {
        Some(Self::write_settings(&self.inner, self.read_only, &update))
    }

    fn stats_report(&self, scope: StatsScope, reply: Reply<StatsScreen>) {
        self.want(scope);
        self.spawn(
            move |inner| Self::stats_screen(inner, scope, Gate::Latest),
            reply,
        );
    }

    fn stats_drill(&self, scope: StatsScope, ask: DrillAsk, reply: Reply<DrillView>) {
        self.want(scope);
        self.spawn(
            move |inner| Self::stats_page(inner, scope, &ask, Gate::Latest),
            reply,
        );
    }

    fn stats_report_now(&self, scope: StatsScope) -> Option<Result<StatsScreen, LoadError>> {
        Some(Self::stats_screen(&self.inner, scope, Gate::Always))
    }

    fn stats_drill_now(
        &self,
        scope: StatsScope,
        ask: &DrillAsk,
    ) -> Option<Result<DrillView, LoadError>> {
        Some(Self::stats_page(&self.inner, scope, ask, Gate::Always))
    }

    fn memory(&self, reply: Reply<MemoryData>) {
        self.spawn(|inner| memory::load(&inner.roots), reply);
    }

    fn memory_write(&self, op: MemoryWrite, reply: Reply<MemoryDone>) {
        self.spawn(
            move |inner| memory::write(&inner.roots, &inner.tmp_dir, op),
            reply,
        );
    }

    fn memory_now(&self) -> Option<Result<MemoryData, LoadError>> {
        Some(memory::load(&self.inner.roots))
    }

    fn memory_write_now(&self, op: &MemoryWrite) -> Option<Result<MemoryDone, LoadError>> {
        Some(memory::write(
            &self.inner.roots,
            &self.inner.tmp_dir,
            op.clone(),
        ))
    }
}
