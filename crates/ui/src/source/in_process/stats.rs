//! The in-process source's stats passes: one corpus pass at a time, a
//! queued pass skipped once the UI asked about another scope, and drill
//! pages sliced from the last drilled row's hits.

use std::sync::Arc;
use std::time::Instant;

use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::service::{
    DrillHits, DrillPage, ReportRequest, StatsEnv, StatsError, drill_hits, report,
};
use peektrace_protocol::stats::DrillView;
use peektrace_protocol::{AgentId, ApiError, DrillTable};
use tracing::{Span, field, info_span};

use super::{InProcess, Inner, locked};
use crate::model::stats::derive::{core_agent, offset_at, text_of};
use crate::model::stats::format::at;
use crate::model::stats::{DrillAsk, StatsScope};
use crate::source::{LoadError, StatsScreen, decode};

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

pub(super) struct Drilled {
    scope: StatsScope,
    table: DrillTable,
    key: String,
    hits: Arc<DrillHits>,
}

/// Whether a pass runs for any scope or only for the last one asked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Gate {
    Always,
    Latest,
}

impl InProcess {
    /// Run `f` on a stats env after earlier corpus passes; under
    /// [`Gate::Latest`], skip it once the UI asked about another scope.
    pub(super) fn with_stats<T>(
        inner: &Inner,
        span: &Span,
        scope: StatsScope,
        gate: Gate,
        f: impl FnOnce(&StatsEnv<'_>) -> Result<T, StatsError>,
    ) -> Result<T, LoadError> {
        let started = Instant::now();
        let _guard = locked(&inner.stats_lock);
        let waited = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        span.record("stats.waitMs", waited);
        if gate == Gate::Latest && *locked(&inner.wanted) != Some(scope) {
            return Err(LoadError::Superseded);
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

    pub(super) fn stats_screen(
        inner: &Inner,
        scope: StatsScope,
        gate: Gate,
    ) -> Result<StatsScreen, LoadError> {
        let span = stats_span!("app.stats.report");
        record_scope(&span, scope);
        let _entered = span.enter();
        let r = Self::with_stats(inner, &span, scope, gate, |env| {
            let r = report(env, &report_request(scope));
            // A new pass may see new calls; the next drill reads them.
            let mut drilled = locked(&inner.drilled);
            if drilled.as_ref().is_some_and(|d| d.scope == scope) {
                *drilled = None;
            }
            r
        })?;
        let out = decode(&r.to_json(false)).map(|view| StatsScreen {
            report: view,
            text: Some(text_of(&r, scope.agent)),
            generated: at(r.generated_at, offset_at(&inner.zone, r.generated_at)),
        });
        if let Err(e) = &out {
            span.record("error", field::display(e));
        }
        out
    }

    fn drilled(inner: &Inner, scope: StatsScope, ask: &DrillAsk) -> Option<Arc<DrillHits>> {
        locked(&inner.drilled)
            .as_ref()
            .filter(|d| d.scope == scope && d.table == ask.table && d.key == ask.key)
            .map(|d| Arc::clone(&d.hits))
    }

    /// Every hit of the row, from the last drill of it or from one pass.
    fn hits(
        inner: &Inner,
        span: &Span,
        scope: StatsScope,
        ask: &DrillAsk,
        gate: Gate,
    ) -> Result<Arc<DrillHits>, LoadError> {
        if let Some(hits) = Self::drilled(inner, scope, ask) {
            return Ok(hits);
        }
        Self::with_stats(inner, span, scope, gate, |env| {
            if let Some(hits) = Self::drilled(inner, scope, ask) {
                return Ok(hits);
            }
            let req = report_request(scope);
            let hits = Arc::new(drill_hits(env, &req, core_table(ask.table), &ask.key)?);
            *locked(&inner.drilled) = Some(Drilled {
                scope,
                table: ask.table,
                key: ask.key.clone(),
                hits: Arc::clone(&hits),
            });
            Ok(hits)
        })
    }

    pub(super) fn stats_page(
        inner: &Inner,
        scope: StatsScope,
        ask: &DrillAsk,
        gate: Gate,
    ) -> Result<DrillView, LoadError> {
        let span = stats_span!("app.stats.drill");
        record_scope(&span, scope);
        span.record(
            "stats.table",
            crate::model::stats::rows::table_name(ask.table),
        );
        span.record("stats.offset", ask.offset);
        let _entered = span.enter();
        let page = DrillPage {
            offset: Some(f64::from(ask.offset)),
            page_size: Some(crate::model::stats::rows::DRILL_PAGE),
        };
        let hits = Self::hits(inner, &span, scope, ask, gate)?;
        decode(&hits.page(page).to_json(false))
    }

    /// Mark `scope` as the one the UI shows now.
    pub(super) fn want(&self, scope: StatsScope) {
        *locked(&self.inner.wanted) = Some(scope);
    }
}

const fn core_table(t: DrillTable) -> peektrace_core::stats::accumulate::DrillTable {
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
