//! `/api` handlers: each reads the request, runs core off the async
//! runtime, and answers JSON or a typed `{"error": ...}`.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, Uri, header};
use base64::Engine as _;
use peektrace_core::capabilities::{self, SupportLevel as CoreLevel};
use peektrace_core::js::json_out::Writer;
use peektrace_core::js::num;
use peektrace_core::js::value::JsValue;
use peektrace_core::sessions::resolve::{self, SessionError};
use peektrace_core::sessions::service::{
    AnalyzeOverrides, SessionEnv, Target, analyze_session_with, get_session, session_image,
};
use peektrace_core::sessions::write::{analyzed_json_with, listing_json, parsed_json};
use peektrace_core::settings_doc::{
    self, SettingsUpdateError, StoredSettings, validate_settings, write_settings_doc,
};
use peektrace_core::{Agent, ListFilter, list_sessions};
use peektrace_protocol::{
    AgentId, AgentScan, ApiError, ApiErrorBody, Capability, CapabilitySupport, FileChangeReason,
    NotPortedFeature, PerAgentSupport, READ_ONLY_REASON, ScannedRoot, ServerInfo, SupportLevel,
};
use tracing::{Instrument as _, Span, field};

use crate::AppState;

pub type Res = Response<Body>;
type Shared = State<Arc<AppState>>;

/// A compact JSON body.
pub fn json(body: String) -> Res {
    let mut res = Response::new(Body::from(body));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}

/// `{"error": ...}` with the error's status.
pub fn api_error(e: ApiError) -> Res {
    let code = StatusCode::from_u16(e.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut res = json(ApiErrorBody::json(e));
    *res.status_mut() = code;
    res
}

fn bad_request(reason: impl Into<String>) -> ApiError {
    ApiError::BadRequestError {
        reason: reason.into(),
    }
}

fn session_error(e: SessionError) -> ApiError {
    match e {
        SessionError::NotFound { id, searched } => ApiError::SessionNotFoundError {
            id,
            searched_root: searched,
        },
        SessionError::Read { path, reason } => ApiError::TranscriptParseError { path, reason },
    }
}

/// Run blocking core work off the runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| Err(bad_request(format!("handler failed: {e}"))))
}

/// The root span of one API call: `rpc.<tag>`, with no parent.
macro_rules! rpc_span {
    ($tag:literal, $method:expr, $route:expr) => {
        tracing::info_span!(
            parent: None,
            concat!("rpc.", $tag),
            peektrace.root = true,
            peektrace.kind = "rpc",
            http.method = %$method,
            http.route = $route,
            http.status = field::Empty,
            sessions.unreadable = field::Empty,
            error = field::Empty
        )
    };
}

/// Answer `result`, recording status and error on the current span.
fn finish(result: Result<Res, ApiError>) -> Res {
    let span = Span::current();
    let res = result.unwrap_or_else(|e| {
        let body = serde_json::to_value(&e).unwrap_or_default();
        span.record("error", field::display(body.get("_tag").unwrap_or(&body)));
        api_error(e)
    });
    span.record("http.status", res.status().as_u16());
    res
}

/// Every query param, decoded, in order.
fn query_pairs(raw: Option<&str>) -> Vec<(String, String)> {
    raw.map(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .into_owned()
            .collect()
    })
    .unwrap_or_default()
}

fn param<'a>(q: &'a [(String, String)], key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn redact_param(q: &[(String, String)]) -> Result<bool, ApiError> {
    match param(q, "redact") {
        None | Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(v) => Err(bad_request(format!(
            "redact must be true or false, got {v:?}"
        ))),
    }
}

fn number_param(q: &[(String, String)], key: &str) -> Result<Option<f64>, ApiError> {
    param(q, key)
        .map(|v| {
            let n = num::parse_number(v);
            if v.is_empty() || !n.is_finite() {
                Err(bad_request(format!(
                    "{key} must be a finite number, got {v:?}"
                )))
            } else {
                Ok(n)
            }
        })
        .transpose()
}

fn env(state: &AppState) -> SessionEnv<'_> {
    SessionEnv {
        roots: &state.roots,
        local: state.zone.as_ref(),
    }
}

pub async fn server_info(State(state): Shared, method: Method) -> Res {
    let span = rpc_span!("server.info", method, "/api/server");
    async move {
        let agents = Agent::ALL
            .into_iter()
            .map(|agent| AgentScan {
                agent: agent_id(agent),
                roots: state
                    .roots
                    .of(agent)
                    .iter()
                    .enumerate()
                    .map(|(i, s)| ScannedRoot {
                        is_default: i == 0,
                        home: s.home.clone(),
                        path: s.projects_root.clone(),
                        exists: resolve::exists(&s.projects_root),
                    })
                    .collect(),
            })
            .collect();
        let info = ServerInfo {
            read_only: state.read_only,
            user_home: state.user_home.clone(),
            agents,
        };
        finish(Ok(json(serde_json::to_string(&info).unwrap_or_default())))
    }
    .instrument(span)
    .await
}

const fn agent_id(a: Agent) -> AgentId {
    match a {
        Agent::Claude => AgentId::Claude,
        Agent::Codex => AgentId::Codex,
        Agent::Pi => AgentId::Pi,
        Agent::Opencode => AgentId::Opencode,
    }
}

const fn level(l: CoreLevel) -> SupportLevel {
    match l {
        CoreLevel::Supported => SupportLevel::Supported,
        CoreLevel::Partial => SupportLevel::Partial,
        CoreLevel::Planned => SupportLevel::Planned,
        CoreLevel::Unsupported => SupportLevel::Unsupported,
    }
}

pub async fn capabilities(method: Method) -> Res {
    let _span = rpc_span!("capabilities.list", method, "/api/capabilities").entered();
    let rows: Vec<Capability> = capabilities::list_capabilities()
        .iter()
        .map(|c| {
            let [claude, codex, pi, opencode] = c.per_agent.map(|s| CapabilitySupport {
                level: level(s.level),
                note: s.note.map(str::to_owned),
            });
            Capability {
                id: c.id.to_owned(),
                group: c.group.to_owned(),
                title: c.title.to_owned(),
                description: c.description.to_owned(),
                per_agent: PerAgentSupport {
                    claude,
                    codex,
                    pi,
                    opencode,
                },
            }
        })
        .collect();
    finish(Ok(json(serde_json::to_string(&rows).unwrap_or_default())))
}

pub async fn sessions(State(state): Shared, method: Method, RawQuery(raw): RawQuery) -> Res {
    let span = rpc_span!("sessions.listing", method, "/api/sessions");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let agent = match param(&q, "agent").filter(|a| !a.is_empty()) {
                None => None,
                Some(a) => Some(
                    Agent::parse(a).ok_or_else(|| bad_request(format!("unknown agent {a:?}")))?,
                ),
            };
            let project = param(&q, "project")
                .filter(|p| !p.is_empty())
                .map(str::to_owned);
            let current = Span::current();
            let listing = blocking(move || {
                let _entered = current.enter();
                Ok(list_sessions(&state.roots, &ListFilter { agent, project }))
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

fn settings_body(s: &StoredSettings) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    write_settings_doc(w.key("settings"), &s.doc);
    w.key("mtimeMs").int(s.mtime_ms);
    w.key("path").str(&s.path);
    w.end_obj();
    w.finish()
}

pub async fn settings_get(State(state): Shared, method: Method) -> Res {
    let span = rpc_span!("settings.get", method, "/api/settings");
    async move {
        let path = state.settings_path.clone();
        let result = blocking(move || Ok(settings_doc::read_stored(&path))).await;
        finish(result.map(|s| json(settings_body(&s))))
    }
    .instrument(span)
    .await
}

fn update_error(e: SettingsUpdateError) -> ApiError {
    match e {
        SettingsUpdateError::Changed { path, reason } => ApiError::FileChangedError {
            path,
            reason: match reason {
                settings_doc::ChangeReason::Mtime => FileChangeReason::Mtime,
                settings_doc::ChangeReason::Missing => FileChangeReason::Missing,
            },
        },
        SettingsUpdateError::Write { path, reason } => {
            ApiError::SettingsWriteError { path, reason }
        }
    }
}

pub async fn settings_put(State(state): Shared, method: Method, body: Bytes) -> Res {
    let span = rpc_span!("settings.update", method, "/api/settings");
    async move {
        let result = async {
            if state.read_only {
                return Err(ApiError::SettingsWriteError {
                    path: "settings.json".to_owned(),
                    reason: READ_ONLY_REASON.to_owned(),
                });
            }
            let text = std::str::from_utf8(&body).map_err(|_| bad_request("body is not UTF-8"))?;
            let value = JsValue::parse(text).ok_or_else(|| bad_request("body is not JSON"))?;
            let doc = validate_settings(
                value
                    .get("settings")
                    .ok_or_else(|| bad_request("body needs settings"))?,
            )
            .map_err(bad_request)?;
            let expected = match value.get("expectedMtime") {
                None => None,
                Some(v) => Some(
                    v.as_num()
                        .ok_or_else(|| bad_request("expectedMtime must be a number"))?,
                ),
            };
            let path = state.settings_path.clone();
            let stored = blocking(move || {
                settings_doc::write_stored(&path, doc, expected).map_err(update_error)
            })
            .await?;
            Ok(json(settings_body(&stored)))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn watch_poll(State(state): Shared, method: Method) -> Res {
    let _span = rpc_span!("watch.poll", method, "/api/watch").entered();
    let v = *state.versions.borrow();
    finish(Ok(json(serde_json::to_string(&v).unwrap_or_default())))
}

/// 501 for a feature this build does not serve yet.
pub fn not_ported(feature: NotPortedFeature) -> Res {
    api_error(ApiError::NotPortedYetError { feature })
}

pub async fn not_found(uri: Uri) -> Res {
    api_error(ApiError::NotFoundError {
        path: uri.path().to_owned(),
    })
}

pub async fn method_not_allowed(method: Method) -> Res {
    api_error(ApiError::MethodNotAllowedError {
        method: method.to_string(),
    })
}
