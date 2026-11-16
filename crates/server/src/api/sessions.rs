//! Session routes: listing, one transcript, its analysis and its images.

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderValue, Method, Response, header};
use base64::Engine as _;
use peektrace_core::sessions::resolve::SessionError;
use peektrace_core::sessions::service::{
    AnalyzeOverrides, SessionEnv, Target, analyze_session_with, get_session, session_image,
};
use peektrace_core::sessions::write::{analyzed_json_with, listing_json, parsed_json};
use peektrace_core::{ListFilter, list_sessions_cached};
use peektrace_protocol::ApiError;
use tracing::{Instrument as _, Span};

use super::params::{agent_param, number_param, param, query_pairs, redact_param};
use super::{Res, Shared, bad_request, blocking, finish, json};
use crate::AppState;

pub(super) fn session_error(e: SessionError) -> ApiError {
    match e {
        SessionError::NotFound { id, searched } => ApiError::SessionNotFoundError {
            id,
            searched_root: searched,
        },
        SessionError::Read { path, reason } => ApiError::TranscriptParseError { path, reason },
    }
}

pub(super) fn env(state: &AppState) -> SessionEnv<'_> {
    SessionEnv {
        roots: &state.roots,
        local: state.zone.as_ref(),
    }
}

pub async fn sessions(State(state): Shared, method: Method, RawQuery(raw): RawQuery) -> Res {
    let span = rpc_span!("sessions.listing", method, "/api/sessions");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let agent = agent_param(&q)?;
            let project = param(&q, "project")
                .filter(|p| !p.is_empty())
                .map(str::to_owned);
            let current = Span::current();
            let listing = blocking(move || {
                let _entered = current.enter();
                Ok::<_, ApiError>(list_sessions_cached(
                    &state.roots,
                    &ListFilter { agent, project },
                    &state.headers,
                ))
            })
            .await?;
            Span::current().record("sessions.unreadable", listing.unreadable.len());
            Ok(json(listing_json(&listing)))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn session(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!("sessions.get", method, "/api/sessions/{id}");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let redact = redact_param(&q)?;
            let parent = param(&q, "parent").map(str::to_owned);
            let current = Span::current();
            let body = blocking(move || {
                let _entered = current.enter();
                get_session(env(&state), Target::Lookup(&id), parent.as_deref(), redact)
                    .map(|p| parsed_json(&p, false))
                    .map_err(session_error)
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

pub async fn analysis(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!("sessions.analyze", method, "/api/sessions/{id}/analysis");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let redact = redact_param(&q)?;
            let overrides = AnalyzeOverrides {
                window: number_param(&q, "window")?,
                dumb_zone: number_param(&q, "dumbZone")?,
            };
            let parent = param(&q, "parent").map(str::to_owned);
            let current = Span::current();
            let body = blocking(move || {
                let _entered = current.enter();
                analyze_session_with(
                    env(&state),
                    Target::Lookup(&id),
                    parent.as_deref(),
                    redact,
                    overrides,
                )
                .map(|a| analyzed_json_with(&a, false))
                .map_err(session_error)
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

fn index(raw: &str, what: &str) -> Result<usize, ApiError> {
    raw.parse().map_err(|_| {
        bad_request(format!(
            "{what} must be a non-negative integer, got {raw:?}"
        ))
    })
}

pub async fn image(
    State(state): Shared,
    method: Method,
    Path(parts): Path<Vec<(String, String)>>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!("sessions.image", method, "/api/sessions/{id}/images");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let get = |k: &str| parts.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            let id = get("id").unwrap_or_default().to_owned();
            let line = index(get("line").unwrap_or_default(), "line")?;
            let block = index(get("block").unwrap_or_default(), "block")?;
            let item = get("item").map(|i| index(i, "item")).transpose()?;
            let parent = param(&q, "parent").map(str::to_owned);
            let current = Span::current();
            let found = blocking(move || {
                let _entered = current.enter();
                let found = session_image(env(&state), &id, parent.as_deref(), line, block, item)
                    .map_err(session_error)?;
                let not_found = || ApiError::ImageNotFoundError {
                    id: id.clone(),
                    line: line as u64,
                    block: block as u64,
                    item: item.map(|i| i as u64),
                };
                let img = found.ok_or_else(not_found)?;
                let clean: String = img.data.chars().filter(|c| !c.is_whitespace()).collect();
                base64::engine::general_purpose::STANDARD
                    .decode(clean)
                    .map(|bytes| (bytes, img.media_type))
                    .map_err(|_| not_found())
            })
            .await?;
            let (bytes, media) = found;
            let mut res = Response::new(Body::from(bytes));
            let headers = res.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&media) {
                headers.insert(header::CONTENT_TYPE, v);
            }
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=300"),
            );
            Ok(res)
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}
