//! The peektrace HTTP server: JSON API under `/api`, live change events,
//! and the web UI, behind Host, Origin and token guards.

// A panic in core work must answer 500, not end the process.
#[cfg(panic = "abort")]
compile_error!("peektrace-server needs panic = \"unwind\"");

mod api;
pub mod assets;
pub mod auth;
pub mod bind;
mod events;
pub mod guard;
pub mod rune_assets;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::routing::{MethodRouter, any, get, post};
use peektrace_core::AgentRoots;
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::js::time::LocalZone;
use peektrace_protocol::WatchVersions;
use tokio::sync::watch;
use tower_http::CompressionLevel;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};

pub use assets::Assets;
use auth::{AuthOutcome, AuthRequest};
use guard::Allowlist;

const UNAUTHORIZED: &str = "Peektrace needs the token: open the token URL printed by peektrace (serve output or tui footer).";

/// What every handler reads.
pub struct AppState {
    pub roots: AgentRoots,
    pub settings_path: String,
    pub user_home: String,
    pub read_only: bool,
    pub zone: Arc<dyn LocalZone + Send + Sync>,
    pub versions: watch::Receiver<WatchVersions>,
    /// Turns true when the server starts to stop; ends `/api/events` streams.
    pub stop: watch::Receiver<bool>,
    /// List headers kept between `/api/sessions` calls.
    pub headers: HeaderCache,
    /// The stats fact cache dir (`$PEEKTRACE_DIR/stats-rs`).
    pub stats_cache_root: String,
    /// Node's `os.tmpdir()`; memory writes may touch it.
    pub tmp_dir: String,
    /// Current time in ms; stats windows end here.
    pub now: Clock,
    /// Held across each corpus-wide stats pass, so they run one at a time.
    pub stats_lock: Arc<tokio::sync::Mutex<()>>,
}

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// The wall clock in ms since the epoch.
pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
            .floor()
    })
}

/// Request guards, fixed once the port is bound.
#[derive(Debug, Clone)]
pub struct Guards {
    pub allow: Allowlist,
    pub token: Option<String>,
    pub port: u16,
}

fn text(code: StatusCode, body: &'static str) -> Response<Body> {
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = code;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain;charset=utf-8"),
    );
    res
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Host guard, then auth, on every request; then Origin on `/api`.
async fn guard(State(g): State<Arc<Guards>>, req: Request, next: Next) -> Response<Body> {
    let authed = g.token.is_some();
    let mut res = guarded(&g, req, next).await;
    if authed {
        res.headers_mut().insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        );
    }
    res
}

async fn guarded(g: &Guards, req: Request, next: Next) -> Response<Body> {
    let headers = req.headers();
    let host = header_str(headers, header::HOST);
    let authed = g.token.is_some();
    if let Some(reason) = g.allow.host_refusal(host, authed) {
        return text(StatusCode::FORBIDDEN, reason);
    }
    if let Some(token) = &g.token {
        let target = auth::served_target(
            req.uri()
                .path_and_query()
                .map_or_else(|| req.uri().path(), |p| p.as_str()),
        );
        let outcome = auth::auth_outcome(
            AuthRequest {
                method: req.method().as_str(),
                target: &target,
                authorization: header_str(headers, header::AUTHORIZATION),
                cookie: header_str(headers, header::COOKIE),
            },
            token,
            g.port,
        );
        match outcome {
            AuthOutcome::Ok => {}
            AuthOutcome::Unauthorized => {
                let mut res = text(StatusCode::UNAUTHORIZED, UNAUTHORIZED);
                res.headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                return res;
            }
            AuthOutcome::Redirect(location) => {
                let mut res = Response::new(Body::empty());
                *res.status_mut() = StatusCode::FOUND;
                let h = res.headers_mut();
                if let Ok(v) = HeaderValue::from_str(&location) {
                    h.insert(header::LOCATION, v);
                }
                if let Ok(v) = HeaderValue::from_str(&auth::set_cookie(g.port, token)) {
                    h.insert(header::SET_COOKIE, v);
                }
                return res;
            }
        }
    }
    let is_api = req.uri().path() == "/api" || req.uri().path().starts_with("/api/");
    if is_api {
        let origin = header_str(headers, header::ORIGIN);
        if let Some(reason) = g.allow.origin_refusal(origin, host, authed) {
            return text(StatusCode::FORBIDDEN, reason);
        }
    }
    next.run(req).await
}

/// A route that answers a typed 405 for other methods.
fn route(r: MethodRouter<Arc<AppState>>) -> MethodRouter<Arc<AppState>> {
    r.fallback(api::method_not_allowed)
}

fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/server", route(get(api::server_info)))
        .route("/api/capabilities", route(get(api::capabilities)))
        .route("/api/sessions", route(get(api::sessions)))
        .route("/api/sessions/search", route(get(api::search)))
        .route("/api/sessions/{id}", route(get(api::session)))
        .route("/api/sessions/{id}/analysis", route(get(api::analysis)))
        .route("/api/sessions/{id}/markers", route(get(api::markers)))
        .route(
            "/api/sessions/{id}/images/{line}/{block}",
            route(get(api::image)),
        )
        .route(
            "/api/sessions/{id}/images/{line}/{block}/{item}",
            route(get(api::image)),
        )
        .route(
            "/api/settings",
            route(get(api::settings_get).put(api::settings_put)),
        )
        .route("/api/watch", route(get(api::watch_poll)))
        .route("/api/events", route(get(events::events)))
        .route("/api/stats/report", route(get(api::stats_report)))
        .route(
            "/api/stats/drill/{table}/{key}",
            route(get(api::stats_drill)),
        )
        .route("/api/stats/usage", route(get(api::stats_usage)))
        .route("/api/stats/refresh", route(post(api::stats_refresh)))
        .route("/api/memory", route(get(api::memory_all)))
        .route("/api/memory/projects", route(get(api::memory_projects)))
        .route(
            "/api/memory/projects/{project}",
            route(get(api::memory_vault)),
        )
        .route(
            "/api/memory/projects/{project}/entries",
            route(post(api::memory_create)),
        )
        .route(
            "/api/memory/projects/{project}/entries/{name}",
            route(axum::routing::patch(api::memory_update).delete(api::memory_delete)),
        )
        .route("/api", any(api::not_found))
        .route("/api/{*rest}", any(api::not_found))
}

/// GET and HEAD of a UI path; other methods are a bare 404.
async fn static_files(State(assets): State<Arc<Assets>>, req: Request) -> Response<Body> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::NOT_FOUND;
        return res;
    }
    let (parts, _) = req.into_parts();
    tokio::task::spawn_blocking(move || assets.respond_to(parts.uri.path(), &parts.headers))
        .await
        .unwrap_or_else(|_| text(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"))
}

async fn health() -> Response<Body> {
    text(StatusCode::OK, "ok")
}

async fn empty_404() -> Response<Body> {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::NOT_FOUND;
    res
}

/// Bodies worth gzipping: not tiny, not a stream, and not already
/// compressed (images other than SVG, fonts).
fn compressible() -> impl Predicate {
    SizeAbove::new(1024)
        .and(NotForContentType::GRPC)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE)
        .and(NotForContentType::const_new("font/"))
}

/// The whole app: API, `/health`, the UI, and the guards in front.
pub fn app(state: Arc<AppState>, assets: Arc<Assets>, guards: Arc<Guards>) -> Router {
    // Level 1: on loopback the CPU cost of a higher level outweighs the bytes saved.
    let compress = CompressionLayer::new()
        .gzip(true)
        .quality(CompressionLevel::Fastest)
        .compress_when(compressible());
    Router::new()
        .route("/health", get(health).fallback(empty_404))
        .merge(api_router().with_state(state))
        .fallback_service(axum::routing::any(static_files).with_state(assets))
        .layer(compress)
        .layer(middleware::from_fn_with_state(guards, guard))
}

/// Serve `app` on `listener` until `shutdown` resolves.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}
