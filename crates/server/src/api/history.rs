//! Session detail in parts: summary, growth series, history index, event
//! pages and history search, all cut from one cached analysis.

use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::Method;
use peektrace_core::sessions::pages::{MAX_LIMIT, PAGE_SIZE, SessionPages};
use peektrace_core::sessions::service::{AnalyzeOverrides, Target, analyze_session_tracked};
use peektrace_protocol::ApiError;
use tracing::{Instrument as _, Span};

use super::params::{Query, number_param, param, query_pairs, redact_param};
use super::sessions::{env, session_error};
use super::{Failure, Res, Shared, bad_request, blocking, finish, json};
use crate::AppState;
use crate::pages::{Built, PagesKey};

fn pages_key(id: String, q: &Query) -> Result<PagesKey, ApiError> {
    Ok(PagesKey {
        redact: redact_param(q)?,
        window: number_param(q, "window")?,
        dumb_zone: number_param(q, "dumbZone")?,
        parent: param(q, "parent").map(str::to_owned),
        id,
    })
}

/// The cached analysis for `key`, analyzed off the runtime on a miss.
async fn load(state: Arc<AppState>, key: PagesKey) -> Result<Arc<Built>, Failure> {
    let version = state.versions.borrow().sessions;
    let current = Span::current();
    let build_key = key.clone();
    let cache = Arc::clone(&state);
    let (built, lookup) = cache
        .pages
        .get(&key, version, move || {
            blocking(move || {
                let _entered = current.enter();
                let k = build_key;
                analyze_session_tracked(
                    env(&state),
                    Target::Lookup(&k.id),
                    k.parent.as_deref(),
                    k.redact,
                    AnalyzeOverrides {
                        window: k.window,
                        dumb_zone: k.dumb_zone,
                    },
                )
                .map(|t| Built::new(SessionPages::new(t.analysis), version, t.inputs))
                .map_err(session_error)
            })
        })
        .await?;
    Span::current().record("sessions.cache", lookup.as_str());
    Ok(built)
}

/// Run `answer` on the cached analysis of the request's session.
async fn with_pages(
    state: Arc<AppState>,
    id: String,
    raw: Option<String>,
    answer: impl FnOnce(&SessionPages) -> Result<String, ApiError> + Send + 'static,
) -> Res {
    let q = query_pairs(raw.as_deref());
    let result = async {
        let key = pages_key(id, &q)?;
        let built = load(state, key).await?;
        let current = Span::current();
        let body = blocking(move || {
            let _entered = current.enter();
            answer(&built.pages)
        })
        .await?;
        Ok(json(body))
    }
    .await;
    finish(result)
}

/// An integer query param in `min..=max`, else `default` when absent.
fn count_param(
    q: &Query,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, ApiError> {
    param(q, key).map_or(Ok(default), |raw| {
        raw.parse::<usize>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| {
                bad_request(if max == usize::MAX {
                    format!("{key} must be a non-negative integer, got {raw:?}")
                } else {
                    format!("{key} must be an integer from {min} to {max}, got {raw:?}")
                })
            })
    })
}

pub async fn summary(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!(
        "sessions.summary",
        method,
        "/api/sessions/{id}/summary",
        sessions.cache = tracing::field::Empty
    );
    with_pages(state, id, raw, |p| Ok(p.summary().to_owned()))
        .instrument(span)
        .await
}

pub async fn growth(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!(
        "sessions.growth",
        method,
        "/api/sessions/{id}/growth",
        sessions.cache = tracing::field::Empty
    );
    with_pages(state, id, raw, |p| Ok(p.growth().to_owned()))
        .instrument(span)
        .await
}

pub async fn history_index(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!(
        "sessions.historyIndex",
        method,
        "/api/sessions/{id}/history/index",
        sessions.cache = tracing::field::Empty,
        history.total = tracing::field::Empty
    );
    with_pages(state, id, raw, |p| {
        Span::current().record("history.total", p.total());
        Ok(p.index().to_owned())
    })
    .instrument(span)
    .await
}

pub async fn history(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!(
        "sessions.history",
        method,
        "/api/sessions/{id}/history",
        sessions.cache = tracing::field::Empty,
        history.offset = tracing::field::Empty,
        history.limit = tracing::field::Empty,
        history.total = tracing::field::Empty
    );
    let q = query_pairs(raw.as_deref());
    let bounds = count_param(&q, "offset", 0, 0, usize::MAX)
        .and_then(|o| Ok((o, count_param(&q, "limit", PAGE_SIZE, 1, MAX_LIMIT)?)));
    let (offset, limit) = match bounds {
        Ok(b) => b,
        Err(e) => return async { finish(Err(e.into())) }.instrument(span).await,
    };
    span.record("history.offset", offset);
    span.record("history.limit", limit);
    with_pages(state, id, raw, move |p| {
        Span::current().record("history.total", p.total());
        Ok(p.page(offset, limit))
    })
    .instrument(span)
    .await
}

pub async fn history_search(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!(
        "sessions.historySearch",
        method,
        "/api/sessions/{id}/history/search",
        sessions.cache = tracing::field::Empty,
        history.matches = tracing::field::Empty,
        history.query_len = tracing::field::Empty
    );
    let q = query_pairs(raw.as_deref());
    let Some(needle) = param(&q, "q").filter(|s| !s.is_empty()).map(str::to_owned) else {
        let e = bad_request("q must be a non-empty string");
        return async { finish(Err(e.into())) }.instrument(span).await;
    };
    span.record("history.query_len", needle.encode_utf16().count());
    with_pages(state, id, raw, move |p| {
        let positions = p.search(&needle);
        Span::current().record("history.matches", positions.len());
        Ok(p.search_json(&positions))
    })
    .instrument(span)
    .await
}
