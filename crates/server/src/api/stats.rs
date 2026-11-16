//! Stats routes: report, drill, markers, usage and refresh. Corpus-wide
//! passes on one server run one at a time, so a second one reuses the
//! first one's cache. Markers read one session and never wait for them.

use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::Method;
use peektrace_core::stats::accumulate::DrillTable;
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::detectors::usage::UsageBy;
use peektrace_core::stats::service::{
    DrillPage, ReportRequest, StatsEnv, StatsError, drill, markers_json, refresh, report,
};
use peektrace_core::stats::usage_service::{MAX_USAGE_DAYS, UsageRequest, usage};
use peektrace_protocol::ApiError;
use tracing::{Instrument as _, Span};

use super::params::{
    Query, agents_param, bool_param, int_param, number_param, object_body, opt_agents, opt_bool,
    param, query_pairs, segment,
};
use super::{Failure, Res, Shared, bad_request, blocking, finish, json};
use crate::AppState;

fn stats_error(e: StatsError) -> ApiError {
    match e {
        StatsError::Scan(reason) => ApiError::StatsScanError { reason, path: None },
        StatsError::RowNotFound { table, key } => ApiError::StatsRowNotFoundError {
            key,
            table: table.to_owned(),
        },
    }
}

/// Stats always redact: `redact=false` is refused, not ignored.
fn check_redact(q: &Query) -> Result<(), ApiError> {
    match param(q, "redact") {
        None | Some("true") => Ok(()),
        Some("false") => Err(bad_request("redact=false is not supported")),
        Some(v) => Err(bad_request(format!("redact must be true, got {v:?}"))),
    }
}

fn report_request(q: &Query) -> Result<ReportRequest, ApiError> {
    check_redact(q)?;
    Ok(ReportRequest {
        agents: agents_param(q)?,
        days: number_param(q, "days")?,
        force: bool_param(q, "force", false)?,
        limit: number_param(q, "limit")?,
    })
}

fn env_of(state: &AppState) -> StatsEnv<'_> {
    StatsEnv {
        roots: &state.roots,
        cache: StatsCache::new(state.stats_cache_root.clone()),
        local: state.zone.as_ref(),
        now_ms: (state.now)(),
        progress: None,
    }
}

/// Run `f` on a stats env once this server's earlier passes are done. The
/// lock is held until `f` returns, even if the client leaves.
async fn locked<T: Send + 'static>(
    state: Arc<AppState>,
    f: impl FnOnce(&StatsEnv<'_>) -> Result<T, StatsError> + Send + 'static,
) -> Result<T, Failure> {
    let started = Instant::now();
    let guard = Arc::clone(&state.stats_lock).lock_owned().await;
    let waited = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Span::current().record("stats.waitMs", waited);
    let current = Span::current();
    blocking(move || {
        let _guard = guard;
        let _entered = current.enter();
        f(&env_of(&state)).map_err(stats_error)
    })
    .await
}

/// Run `f` on a stats env now, beside any running pass.
async fn unlocked<T: Send + 'static>(
    state: Arc<AppState>,
    f: impl FnOnce(&StatsEnv<'_>) -> Result<T, StatsError> + Send + 'static,
) -> Result<T, Failure> {
    let current = Span::current();
    blocking(move || {
        let _entered = current.enter();
        f(&env_of(&state)).map_err(stats_error)
    })
    .await
}

pub async fn stats_report(State(state): Shared, method: Method, RawQuery(raw): RawQuery) -> Res {
    let span = rpc_span!(
        "stats.report",
        method,
        "/api/stats/report",
        stats.waitMs = tracing::field::Empty
    );
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let req = report_request(&q)?;
            let body = locked(state, move |env| Ok(report(env, &req)?.to_json(false))).await?;
            Ok(json(body))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn stats_drill(
    State(state): Shared,
    method: Method,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!(
        "stats.drill",
        method,
        "/api/stats/drill/{table}/{key}",
        stats.waitMs = tracing::field::Empty
    );
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let (table, key) = segment(path)?;
            let table = DrillTable::parse(&table)
                .ok_or_else(|| bad_request(format!("unknown drill table {table:?}")))?;
            let req = report_request(&q)?;
            let page = DrillPage {
                offset: number_param(&q, "offset")?,
                page_size: number_param(&q, "pageSize")?,
            };
            let body = locked(state, move |env| {
                Ok(drill(env, &req, table, &key, page)?.to_json(false))
            })
            .await?;
            Ok(json(body))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn markers(
    State(state): Shared,
    method: Method,
    path: Result<Path<String>, PathRejection>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!("stats.markers", method, "/api/sessions/{id}/markers");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let sid = segment(path)?;
            let agents = agents_param(&q)?;
            let force = bool_param(&q, "force", false)?;
            let body = unlocked(state, move |env| {
                let found =
                    peektrace_core::stats::service::markers(env, &sid, agents.as_deref(), force)?;
                Ok(markers_json(&sid, &found, false))
            })
            .await?;
            Ok(json(body))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

fn usage_request(q: &Query) -> Result<UsageRequest, ApiError> {
    let by = param(q, "by").unwrap_or_default();
    let by = UsageBy::parse(by)
        .ok_or_else(|| bad_request(format!("by must be day, month or block, got {by:?}")))?;
    #[expect(clippy::cast_precision_loss, reason = "a small constant")]
    let max = MAX_USAGE_DAYS as f64;
    #[expect(clippy::cast_possible_truncation, reason = "a whole number in range")]
    let days = int_param(q, "days", 1.0..=max)?.map(|d| d as i64);
    Ok(UsageRequest {
        agents: agents_param(q)?,
        by,
        days,
        force: bool_param(q, "force", false)?,
        tz: Some(param(q, "tz").unwrap_or("UTC").to_owned()),
    })
}

pub async fn stats_usage(State(state): Shared, method: Method, RawQuery(raw): RawQuery) -> Res {
    let span = rpc_span!(
        "stats.usage",
        method,
        "/api/stats/usage",
        stats.waitMs = tracing::field::Empty
    );
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let req = usage_request(&q)?;
            let body = locked(state, move |env| Ok(usage(env, &req)?.to_json(false))).await?;
            Ok(json(body))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn stats_refresh(State(state): Shared, method: Method, body: Bytes) -> Res {
    let span = rpc_span!(
        "stats.refresh",
        method,
        "/api/stats/refresh",
        stats.waitMs = tracing::field::Empty
    );
    async move {
        let result = async {
            let o = object_body(&body, true)?;
            let agents = opt_agents(&o, "agents")?;
            let force = opt_bool(&o, "force")?.unwrap_or(false);
            let body = locked(state, move |env| {
                Ok(refresh(env, agents.as_deref(), force)?.to_json(false))
            })
            .await?;
            Ok(json(body))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}
