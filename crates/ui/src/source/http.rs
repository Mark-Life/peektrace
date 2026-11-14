//! The web source: same-origin `fetch` against `/api`, live versions from
//! the `/api/events` stream, and a `/api/watch` poll once the stream fails.

use std::cell::Cell;
use std::rc::Rc;

use peektrace_protocol::stats::DrillView;
use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{Capability, ServerInfo, SettingsResult, SettingsUpdate, WatchVersions};
use tracing::{field, info_span};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{EventSource, MessageEvent, Request, RequestCredentials, RequestInit, Response};

use super::{
    DataSource, LoadError, OnVersions, Reply, StatsScreen, WatchEvent, decode, decode_error,
};
use crate::model::stats::format::at;
use crate::model::stats::rows::{DRILL_PAGE, table_name};
use crate::model::stats::{DrillAsk, StatsScope};

/// Stream errors without an open between them before polling takes over.
const STREAM_TRIES: u32 = 3;
const POLL_MS: i32 = 1000;

#[derive(Debug, Default)]
pub struct Http;

fn js_text(v: &wasm_bindgen::JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

/// GET `path` and return `(status, body)`.
async fn get(path: &str) -> Result<(u16, String), LoadError> {
    send("GET", path, None).await
}

/// Send `method` to `path` with an optional JSON `body`; `(status, body)`.
async fn send(method: &str, path: &str, body: Option<String>) -> Result<(u16, String), LoadError> {
    let transport = |e: wasm_bindgen::JsValue| LoadError::Transport {
        status: None,
        message: js_text(&e),
    };
    let window = web_sys::window().ok_or_else(|| LoadError::Transport {
        status: None,
        message: "no window".into(),
    })?;
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    let has_body = body.is_some();
    if let Some(b) = body {
        init.set_body(&wasm_bindgen::JsValue::from_str(&b));
    }
    let req = Request::new_with_str_and_init(path, &init).map_err(transport)?;
    req.headers()
        .set("accept", "application/json")
        .map_err(transport)?;
    if has_body {
        req.headers()
            .set("content-type", "application/json")
            .map_err(transport)?;
    }
    let resp: Response = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(transport)?
        .unchecked_into();
    let text = JsFuture::from(resp.text().map_err(transport)?)
        .await
        .map_err(transport)?;
    Ok((resp.status(), text.as_string().unwrap_or_default()))
}

async fn load<T: serde::de::DeserializeOwned>(
    route: &'static str,
    path: String,
) -> Result<T, LoadError> {
    load_with(route, "GET", path, None).await
}

async fn load_with<T: serde::de::DeserializeOwned>(
    route: &'static str,
    method: &'static str,
    path: String,
    body: Option<String>,
) -> Result<T, LoadError> {
    let span = info_span!(
        "app.fetch",
        http.route = route,
        http.status = field::Empty,
        error = field::Empty
    );
    let out = match send(method, &path, body).await {
        Ok((status, body)) => {
            span.record("http.status", status);
            if (200..300).contains(&status) {
                decode(&body)
            } else {
                Err(decode_error(status, &body))
            }
        }
        Err(e) => Err(e),
    };
    if let Err(e) = &out {
        span.record("error", field::display(e));
    }
    out
}

/// The browser's offset east of UTC at `ms`, as `Date` reads it.
fn local_offset(ms: f64) -> f64 {
    -js_sys::Date::new(&ms.into()).get_timezone_offset() * 60_000.0
}

fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_default()
}

/// `days=<d>[&agent=<id>]`, the agent left out for every agent.
fn scope_query(scope: StatsScope) -> String {
    let agent = scope
        .agent
        .map(|a| format!("&agent={}", a.as_str()))
        .unwrap_or_default();
    format!("days={}{agent}", scope.days)
}

/// Poll `/api/watch` every second. `recovering`: the first answer after a
/// stream failure also reports a reconnect.
fn poll(on_versions: Rc<OnVersions>, recovering: bool) {
    spawn_local(async move {
        let mut recovering = recovering;
        if let Ok((200, body)) = get("/api/watch").await
            && let Ok(v) = serde_json::from_str::<WatchVersions>(&body)
        {
            on_versions(WatchEvent::Versions(v));
            if recovering {
                recovering = false;
                on_versions(WatchEvent::Reconnected);
            }
        }
        let next = Closure::once_into_js(move || poll(on_versions, recovering));
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                next.unchecked_ref(),
                POLL_MS,
            );
        }
    });
}

/// Stream state between its events.
#[derive(Debug, Default)]
struct Stream {
    /// Errors since the last open.
    failures: Cell<u32>,
    /// An error happened; the next versions also report a reconnect.
    recovering: Cell<bool>,
}

impl DataSource for Http {
    fn sessions(&self, reply: Reply<SessionListingView>) {
        spawn_local(async move { reply(load("/api/sessions", "/api/sessions".into()).await) });
    }

    fn analysis(&self, id: String, parent: Option<String>, reply: Reply<AnalyzedSession>) {
        let parent = parent
            .map(|p| format!("&parent={}", encode(&p)))
            .unwrap_or_default();
        let path = format!("/api/sessions/{}/analysis?redact=true{parent}", encode(&id));
        spawn_local(async move { reply(load("/api/sessions/{id}/analysis", path).await) });
    }

    fn stats_report(&self, scope: StatsScope, reply: Reply<StatsScreen>) {
        let path = format!("/api/stats/report?{}", scope_query(scope));
        spawn_local(async move {
            let out = load("/api/stats/report", path).await;
            reply(
                out.map(|report: peektrace_protocol::stats::StatsReportView| {
                    let generated = at(report.generated_at, local_offset(report.generated_at));
                    StatsScreen {
                        report,
                        text: None,
                        generated,
                    }
                }),
            );
        });
    }

    fn stats_drill(&self, scope: StatsScope, ask: DrillAsk, reply: Reply<DrillView>) {
        let path = format!(
            "/api/stats/drill/{}/{}?{}&offset={}&pageSize={DRILL_PAGE}",
            table_name(ask.table),
            encode(&ask.key),
            scope_query(scope),
            ask.offset
        );
        spawn_local(async move { reply(load("/api/stats/drill/{table}/{key}", path).await) });
    }

    fn capabilities(&self, reply: Reply<Vec<Capability>>) {
        spawn_local(
            async move { reply(load("/api/capabilities", "/api/capabilities".into()).await) },
        );
    }

    fn server_info(&self, reply: Reply<ServerInfo>) {
        spawn_local(async move { reply(load("/api/server", "/api/server".into()).await) });
    }

    fn settings(&self, reply: Reply<SettingsResult>) {
        spawn_local(async move { reply(load("/api/settings", "/api/settings".into()).await) });
    }

    fn save_settings(&self, update: SettingsUpdate, reply: Reply<SettingsResult>) {
        let body = serde_json::to_string(&update).unwrap_or_default();
        spawn_local(async move {
            reply(load_with("/api/settings", "PUT", "/api/settings".into(), Some(body)).await);
        });
    }

    fn watch(&self, on_versions: OnVersions) {
        let on_versions = Rc::new(on_versions);
        let Ok(es) = EventSource::new("/api/events") else {
            poll(on_versions, false);
            return;
        };
        let stream = Rc::new(Stream::default());
        let open_stream = Rc::clone(&stream);
        let on_open = Closure::<dyn FnMut()>::new(move || open_stream.failures.set(0));
        let _ = es.add_event_listener_with_callback("open", on_open.as_ref().unchecked_ref());
        on_open.forget();
        let deliver = Rc::clone(&on_versions);
        let message_stream = Rc::clone(&stream);
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            let Some(v) = ev
                .data()
                .as_string()
                .and_then(|d| serde_json::from_str::<WatchVersions>(&d).ok())
            else {
                return;
            };
            deliver(WatchEvent::Versions(v));
            if message_stream.recovering.replace(false) {
                deliver(WatchEvent::Reconnected);
            }
        });
        let _ =
            es.add_event_listener_with_callback("versions", on_message.as_ref().unchecked_ref());
        on_message.forget();
        let source = es.clone();
        let on_error = Closure::<dyn FnMut()>::new(move || {
            stream.failures.set(stream.failures.get() + 1);
            stream.recovering.set(true);
            // The browser stops retrying after an HTTP error: CLOSED.
            if stream.failures.get() >= STREAM_TRIES || source.ready_state() == EventSource::CLOSED
            {
                let _span =
                    info_span!("app.watch.events", error = "stream failed; polling").entered();
                source.close();
                poll(Rc::clone(&on_versions), true);
            }
        });
        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    }
}
