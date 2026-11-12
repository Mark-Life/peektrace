//! `GET /api/events`: the versions now, then on every change, plus a
//! heartbeat comment every 15 s. The stream ends when the server stops, so
//! an open tab never holds up shutdown.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderValue, Method, header};
use axum::response::IntoResponse as _;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use tokio::sync::watch;

use crate::AppState;
use crate::api::Res;

const HEARTBEAT: Duration = Duration::from_secs(15);

fn versions_event(rx: &watch::Receiver<peektrace_protocol::WatchVersions>) -> Event {
    let v = *rx.borrow();
    Event::default()
        .event("versions")
        .data(serde_json::to_string(&v).unwrap_or_default())
}

/// Resolves once `stop` turns true or its sender is gone.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|s| *s).await;
}

/// The current versions first, then the latest after each change, until
/// `stop`. A slow reader sees only the newest value.
fn version_stream(
    rx: watch::Receiver<peektrace_protocol::WatchVersions>,
    stop: watch::Receiver<bool>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold((rx, stop, true), |(mut rx, mut stop, first)| async move {
        if *stop.borrow() {
            return None;
        }
        if !first {
            tokio::select! {
                changed = rx.changed() => changed.ok()?,
                () = stopped(&mut stop) => return None,
            }
        }
        let event = versions_event(&rx);
        rx.mark_unchanged();
        Some((Ok(event), (rx, stop, false)))
    })
}

pub async fn events(State(state): State<Arc<AppState>>, method: Method) -> Res {
    let span = tracing::info_span!(
        parent: None,
        "rpc.watch.events",
        peektrace.root = true,
        peektrace.kind = "rpc",
        http.method = %method,
        http.route = "/api/events",
        http.status = 200
    );
    let _entered = span.enter();
    let sse = Sse::new(version_stream(state.versions.clone(), state.stop.clone()))
        .keep_alive(KeepAlive::new().interval(HEARTBEAT).text("ping"));
    let mut res = sse.into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}
