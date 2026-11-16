//! `/api` handlers: each reads the request, runs core off the async
//! runtime, and answers JSON or a typed `{"error": ...}`.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Method, Response, StatusCode, Uri, header};
use peektrace_core::wire;
use peektrace_protocol::{ApiError, ApiErrorBody};
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

mod history;
mod memory;
mod params;
mod search;
mod sessions;
mod settings;
mod stats;

pub use history::{growth, history, history_index, history_search, summary};
pub use memory::{
    memory_all, memory_create, memory_delete, memory_projects, memory_update, memory_vault,
};
pub use search::search;
pub use sessions::{analysis, export, image, session, sessions};
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
        let info = wire::server_info(&state.roots, state.read_only, &state.user_home);
        finish(Ok(json(serde_json::to_string(&info).unwrap_or_default())))
    }
    .instrument(span)
    .await
}

pub async fn capabilities(method: Method) -> Res {
    let _span = rpc_span!("capabilities.list", method, "/api/capabilities").entered();
    let rows = wire::capability_rows();
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
