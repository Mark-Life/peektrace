//! `/api` handlers: each reads the request, runs core off the async
//! runtime, and answers JSON or a typed `{"error": ...}`.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Method, Response, StatusCode, Uri, header};
use peektrace_core::Agent;
use peektrace_core::capabilities::{self, SupportLevel as CoreLevel};
use peektrace_core::sessions::resolve;
use peektrace_protocol::{
    AgentId, AgentScan, ApiError, ApiErrorBody, Capability, CapabilitySupport, PerAgentSupport,
    ScannedRoot, ServerInfo, SupportLevel,
};
use tracing::{Instrument as _, Span};

use crate::AppState;

/// The root span of one API call: `rpc.<tag>`, with no parent. Extra
/// fields follow the route.
macro_rules! rpc_span {
    ($tag:literal, $method:expr, $route:expr $(, $($extra:tt)+)?) => {
        tracing::info_span!(
            parent: None,
            concat!("rpc.", $tag),
            peektrace.root = true,
            peektrace.kind = "rpc",
            http.method = %$method,
            http.route = $route,
            http.status = tracing::field::Empty,
            sessions.unreadable = tracing::field::Empty,
            error = tracing::field::Empty
            $(, $($extra)+)?
        )
    };
}

mod memory;
mod params;
mod search;
mod sessions;
mod settings;
mod stats;

pub use memory::{
    memory_all, memory_create, memory_delete, memory_projects, memory_update, memory_vault,
};
pub use search::search;
pub use sessions::{analysis, image, session, sessions};
pub use settings::{settings_get, settings_put};
pub use stats::{markers, stats_drill, stats_refresh, stats_report, stats_usage};

pub type Res = Response<Body>;
pub(crate) type Shared = State<Arc<AppState>>;

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

pub(crate) fn bad_request(reason: impl Into<String>) -> ApiError {
    ApiError::BadRequestError {
        reason: reason.into(),
    }
}

/// Why a handler did not answer 2xx: a typed client-facing error, or a
/// server bug (a panic in core work).
#[derive(Debug)]
pub enum Failure {
    Api(ApiError),
    Defect(String),
}

impl From<ApiError> for Failure {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

const DEFECT: &str = "Internal server error";

/// Run blocking core work off the runtime. A panic there is a defect, and
/// marks the entered root as one.
pub(crate) async fn blocking<T: Send + 'static, E: Into<Failure> + Send + 'static>(
    f: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, Failure> {
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r.map_err(Into::into),
        Err(e) if e.is_panic() => {
            let payload = e.into_panic();
            peektrace_telemetry::record_panic(payload.as_ref());
            Err(Failure::Defect(peektrace_telemetry::layer::panic_message(
                payload.as_ref(),
            )))
        }
        Err(e) => Err(Failure::Defect(e.to_string())),
    }
}

/// The wire `_tag` of `e`, as plain text.
fn error_tag(e: &ApiError) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("_tag")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "Error".to_owned())
}

/// Answer `result`, recording status and error on the current span.
pub(crate) fn finish(result: Result<Res, Failure>) -> Res {
    let span = Span::current();
    let res = result.unwrap_or_else(|f| match f {
        Failure::Api(e) => {
            span.record("error", error_tag(&e).as_str());
            api_error(e)
        }
        Failure::Defect(reason) => {
            tracing::error!(reason = %reason, "handler panicked");
            span.record("error", "Defect");
            let mut res = Response::new(Body::from(DEFECT));
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain;charset=utf-8"),
            );
            res
        }
    });
    span.record("http.status", res.status().as_u16());
    res
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

pub(crate) const fn agent_id(a: Agent) -> AgentId {
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

pub async fn watch_poll(State(state): Shared, method: Method) -> Res {
    let _span = rpc_span!("watch.poll", method, "/api/watch").entered();
    let v = *state.versions.borrow();
    finish(Ok(json(serde_json::to_string(&v).unwrap_or_default())))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panic_is_a_500_defect() {
        let r = blocking::<(), ApiError>(|| panic!("boom")).await;
        let res = finish(r.map(|()| json(String::new())));
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let r = blocking::<(), ApiError>(|| Err(bad_request("x"))).await;
        let res = finish(r.map(|()| json(String::new())));
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn error_tag_is_the_bare_tag() {
        let e = ApiError::SessionNotFoundError {
            id: "nope".to_owned(),
            searched_root: "/r".to_owned(),
        };
        assert_eq!(error_tag(&e), "SessionNotFoundError");
    }
}
