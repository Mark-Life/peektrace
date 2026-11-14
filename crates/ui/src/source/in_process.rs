//! The native source: core runs on a worker thread per call, never on the
//! UI thread, under a root `app.<method>` span.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use peektrace_core::PathStyle;
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::system_zone::SystemZone;
use peektrace_core::roots::{self, RootsEnv};
use peektrace_core::sessions::resolve::SessionError;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::sessions::write::{analyzed_json_with, listing_json};
use peektrace_core::stats::cache::{StatsCache, cache_root};
use peektrace_core::stats::service::{
    DrillPage, ReportRequest, StatsEnv, StatsError, drill, markers, markers_json, refresh, report,
};
use peektrace_core::stats::usage_service::{UsageRequest, usage};
use peektrace_core::watch::{WatchProblem, Watcher};
use peektrace_core::{AgentRoots, ListFilter, list_sessions_cached};
use peektrace_protocol::stats::{DrillView, MarkersView, RefreshView, UsageView};
use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{AgentId, ApiError, RefreshBody, UsageBy, UsageQuery, WatchVersions};
use tracing::{Span, field, info_span};

use super::{DataSource, LoadError, OnVersions, Reply, StatsScreen, WatchEvent, decode};
use crate::model::stats::derive::{core_agent, offset_at, text_of};
use crate::model::stats::{DrillAsk, StatsScope};

struct Inner {
    roots: AgentRoots,
    headers: HeaderCache,
    zone: SystemZone,
    stats_cache: String,
    /// One corpus pass at a time, so a second one reads the first one's cache.
    stats_lock: Mutex<()>,
    /// A fixed clock for headless runs and tests.
    now: Option<f64>,
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

macro_rules! stats_span {
    ($name:literal) => {
        info_span!(
            parent: None,
            $name,
            peektrace.root = true,
            peektrace.kind = "app",
            stats.days = field::Empty,
            stats.agent = field::Empty,
            stats.table = field::Empty,
            stats.offset = field::Empty,
            stats.waitMs = field::Empty,
            session.id = field::Empty,
            error = field::Empty
        )
    };
}

fn stats_error(e: StatsError) -> LoadError {
    LoadError::Api(match e {
        StatsError::Scan(reason) => ApiError::StatsScanError { reason, path: None },
        StatsError::RowNotFound { table, key } => ApiError::StatsRowNotFoundError {
            key,
            table: table.to_owned(),
        },
    })
}

fn agents_of(agent: Option<AgentId>) -> Option<Vec<peektrace_core::Agent>> {
    agent.map(|a| vec![core_agent(a)])
}

fn record_scope(span: &Span, scope: StatsScope) {
    span.record("stats.days", scope.days);
    span.record("stats.agent", scope.agent.map_or("", AgentId::as_str));
}

fn report_request(scope: StatsScope) -> ReportRequest {
    ReportRequest {
        agents: agents_of(scope.agent),
        days: Some(f64::from(scope.days)),
        force: false,
        limit: None,
    }
}

fn number(s: Option<&String>) -> Option<f64> {
    s.map(|v| peektrace_core::js::num::parse_number(v))
        .filter(|v| v.is_finite())
}

#[expect(clippy::cast_possible_truncation, reason = "a whole day count")]
fn usage_request(q: &UsageQuery) -> UsageRequest {
    use peektrace_core::stats::detectors::usage::UsageBy as CoreBy;
    UsageRequest {
        agents: q.agent.as_ref().map(|a| {
            a.iter()
                .filter_map(|s| AgentId::parse(s))
                .map(core_agent)
                .collect()
        }),
        by: match q.by {
            UsageBy::Day => CoreBy::Day,
            UsageBy::Month => CoreBy::Month,
            UsageBy::Block => CoreBy::Block,
        },
        days: number(q.days.as_ref()).map(|d| d as i64),
        force: q.force.as_deref() == Some("true"),
        tz: Some(q.tz.clone().unwrap_or_else(|| "UTC".to_owned())),
    }
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
    /// Sessions under `roots`; the stats fact cache in `stats_cache`.
    pub fn new(roots: AgentRoots, stats_cache: String) -> Self {
        Self {
            inner: Arc::new(Inner {
                roots,
                headers: HeaderCache::default(),
                zone: SystemZone::default(),
                stats_cache,
                stats_lock: Mutex::new(()),
                now: crate::fixed_now(),
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
        Self::new(AgentRoots::resolve(&env, &settings), cache_root(&env))
    }

    /// Run `f` on a stats env; `locked` waits for earlier corpus passes.
    fn with_stats<T>(
        inner: &Inner,
        span: &Span,
        locked: bool,
        f: impl FnOnce(&StatsEnv<'_>) -> Result<T, StatsError>,
    ) -> Result<T, LoadError> {
        let started = Instant::now();
        let _guard = locked.then(|| {
            inner
                .stats_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        });
        if locked {
            let waited = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            span.record("stats.waitMs", waited);
        }
        let env = StatsEnv {
            roots: &inner.roots,
            cache: StatsCache::new(inner.stats_cache.clone()),
            local: &inner.zone,
            now_ms: inner.now.unwrap_or_else(crate::wall_ms),
            progress: None,
        };
        let out = f(&env).map_err(stats_error);
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
    }

    fn stats_screen(inner: &Inner, scope: StatsScope) -> Result<StatsScreen, LoadError> {
        let span = stats_span!("app.stats.report");
        record_scope(&span, scope);
        let _entered = span.enter();
        let r = Self::with_stats(inner, &span, true, |env| {
            report(env, &report_request(scope))
        })?;
        let out = decode(&r.to_json(false)).map(|view| StatsScreen {
            report: view,
            text: Some(text_of(
                &r,
                scope.agent,
                offset_at(&inner.zone, r.generated_at),
            )),
        });
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
    }

    fn stats_page(
        inner: &Inner,
        scope: StatsScope,
        ask: &DrillAsk,
    ) -> Result<DrillView, LoadError> {
        let span = stats_span!("app.stats.drill");
        record_scope(&span, scope);
        span.record(
            "stats.table",
            crate::model::stats::rows::table_name(ask.table),
        );
        span.record("stats.offset", ask.offset);
        let _entered = span.enter();
        let table = core_table(ask.table);
        let page = DrillPage {
            offset: Some(f64::from(ask.offset)),
            page_size: Some(crate::model::stats::rows::DRILL_PAGE),
        };
        let body = Self::with_stats(inner, &span, true, |env| {
            Ok(drill(env, &report_request(scope), table, &ask.key, page)?.to_json(false))
        })?;
        decode(&body)
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

    fn stats_report(&self, scope: StatsScope, reply: Reply<StatsScreen>) {
        self.spawn(move |inner| Self::stats_screen(inner, scope), reply);
    }

    fn stats_drill(&self, scope: StatsScope, ask: DrillAsk, reply: Reply<DrillView>) {
        self.spawn(move |inner| Self::stats_page(inner, scope, &ask), reply);
    }

    fn stats_usage(&self, query: UsageQuery, reply: Reply<UsageView>) {
        self.spawn(
            move |inner| {
                let span = stats_span!("app.stats.usage");
                let _entered = span.enter();
                let req = usage_request(&query);
                let body = Self::with_stats(inner, &span, true, |env| {
                    Ok(usage(env, &req)?.to_json(false))
                })?;
                decode(&body)
            },
            reply,
        );
    }

    fn stats_refresh(&self, body: RefreshBody, reply: Reply<RefreshView>) {
        self.spawn(
            move |inner| {
                let span = stats_span!("app.stats.refresh");
                let _entered = span.enter();
                let agents: Option<Vec<_>> =
                    body.agents.map(|a| a.into_iter().map(core_agent).collect());
                let force = body.force.unwrap_or(false);
                let out = Self::with_stats(inner, &span, true, |env| {
                    Ok(refresh(env, agents.as_deref(), force)?.to_json(false))
                })?;
                decode(&out)
            },
            reply,
        );
    }

    fn markers(&self, sid: String, agent: Option<AgentId>, reply: Reply<MarkersView>) {
        self.spawn(
            move |inner| {
                let span = stats_span!("app.stats.markers");
                span.record("session.id", sid.as_str());
                let _entered = span.enter();
                let agents = agents_of(agent);
                let body = Self::with_stats(inner, &span, false, |env| {
                    let found = markers(env, &sid, agents.as_deref(), false)?;
                    Ok(markers_json(&sid, &found, false))
                })?;
                decode(&body)
            },
            reply,
        );
    }

    fn stats_report_now(&self, scope: StatsScope) -> Option<Result<StatsScreen, LoadError>> {
        Some(Self::stats_screen(&self.inner, scope))
    }

    fn stats_drill_now(
        &self,
        scope: StatsScope,
        ask: &DrillAsk,
    ) -> Option<Result<DrillView, LoadError>> {
        Some(Self::stats_page(&self.inner, scope, ask))
    }
}

const fn core_table(
    t: peektrace_protocol::DrillTable,
) -> peektrace_core::stats::accumulate::DrillTable {
    use peektrace_core::stats::accumulate::DrillTable as Core;
    use peektrace_protocol::DrillTable as Wire;
    match t {
        Wire::Cause => Core::Cause,
        Wire::Fail => Core::Fail,
        Wire::Time => Core::Time,
        Wire::Cluster => Core::Cluster,
        Wire::Wait => Core::Wait,
    }
}
