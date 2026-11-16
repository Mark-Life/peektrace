//! Session routes: listing, one transcript, its analysis and its images.

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderValue, Method, Response, header};
use base64::Engine as _;
use peektrace_core::export::{ExportFormat, ExportOptions, render_export};
use peektrace_core::js::{text, time};
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

fn export_format(raw: Option<&str>) -> Result<ExportFormat, ApiError> {
    raw.and_then(ExportFormat::parse).ok_or_else(|| {
        bad_request(format!(
            "format must be one of html, md, json, got {}",
            raw.map_or_else(|| "nothing".to_owned(), |f| format!("{f:?}"))
        ))
    })
}

const fn export_mime(f: ExportFormat) -> &'static str {
    match f {
        ExportFormat::Html => "text/html;charset=utf-8",
        ExportFormat::Md => "text/markdown;charset=utf-8",
        ExportFormat::Json => "application/json;charset=utf-8",
    }
}

/// `GET /api/sessions/{id}/export?format=html|md|json`: the session as a
/// download, with the same redaction as `sessions export`.
pub async fn export(
    State(state): Shared,
    method: Method,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Res {
    let span = rpc_span!("sessions.export", method, "/api/sessions/{id}/export");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let format = export_format(param(&q, "format"))?;
            let redact = redact_param(&q)?;
            let parent = param(&q, "parent").map(str::to_owned);
            let current = Span::current();
            let (name, body) = blocking(move || {
                let _entered = current.enter();
                let session =
                    get_session(env(&state), Target::Lookup(&id), parent.as_deref(), redact)
                        .map_err(session_error)?;
                #[expect(clippy::cast_possible_truncation, reason = "epoch ms fit i64")]
                let exported_at = time::to_iso_string((state.now)() as i64);
                let opts = ExportOptions {
                    exported_at,
                    home: Some(state.user_home.clone()),
                    redacted: redact,
                };
                let text = text::to_display(&render_export(format, &session, &opts)).into_owned();
                let short: String = session.session_id.chars().take(8).collect();
                Ok::<_, ApiError>((format!("session-{short}.{}", format.as_str()), text))
            })
            .await?;
            let mut res = Response::new(Body::from(body));
            let headers = res.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(export_mime(format)),
            );
            if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
                headers.insert(header::CONTENT_DISPOSITION, v);
            }
            Ok(res)
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
