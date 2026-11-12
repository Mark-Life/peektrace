//! The web source: same-origin `fetch` against `/api`, live versions from
//! the `/api/events` stream, and a `/api/watch` poll once the stream fails.

use std::cell::Cell;
use std::rc::Rc;

use peektrace_protocol::WatchVersions;
use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use tracing::{field, info_span};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{EventSource, MessageEvent, Request, RequestCredentials, RequestInit, Response};

use super::{DataSource, LoadError, OnVersions, Reply, decode, decode_error};

/// Stream errors in a row before polling takes over.
const STREAM_TRIES: u32 = 3;
const POLL_MS: i32 = 1000;

#[derive(Debug, Default)]
pub struct Http;

fn js_text(v: &wasm_bindgen::JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

/// GET `path` and return `(status, body)`.
async fn get(path: &str) -> Result<(u16, String), LoadError> {
    let transport = |e: wasm_bindgen::JsValue| LoadError::Transport {
        status: None,
        message: js_text(&e),
    };
    let window = web_sys::window().ok_or_else(|| LoadError::Transport {
        status: None,
        message: "no window".into(),
    })?;
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_credentials(RequestCredentials::SameOrigin);
    let req = Request::new_with_str_and_init(path, &init).map_err(transport)?;
    req.headers()
        .set("accept", "application/json")
        .map_err(transport)?;
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
    let span = info_span!(
        "app.fetch",
        http.route = route,
        http.status = field::Empty,
        error = field::Empty
    );
    let out = match get(&path).await {
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

fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_default()
}

fn poll(on_versions: Rc<OnVersions>) {
    spawn_local(async move {
        if let Ok((200, body)) = get("/api/watch").await
            && let Ok(v) = serde_json::from_str::<WatchVersions>(&body)
        {
            on_versions(v);
        }
        let next = Closure::once_into_js(move || poll(on_versions));
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                next.unchecked_ref(),
                POLL_MS,
            );
        }
    });
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

    fn watch(&self, on_versions: OnVersions) {
        let on_versions = Rc::new(on_versions);
        let Ok(es) = EventSource::new("/api/events") else {
            poll(on_versions);
            return;
        };
        let deliver = Rc::clone(&on_versions);
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            if let Some(v) = ev
                .data()
                .as_string()
                .and_then(|d| serde_json::from_str::<WatchVersions>(&d).ok())
            {
                deliver(v);
            }
        });
        let _ =
            es.add_event_listener_with_callback("versions", on_message.as_ref().unchecked_ref());
        on_message.forget();
        let errors = Rc::new(Cell::new(0_u32));
        let source = es.clone();
        let on_error = Closure::<dyn FnMut()>::new(move || {
            errors.set(errors.get() + 1);
            if errors.get() == STREAM_TRIES {
                let _span =
                    info_span!("app.watch.events", error = "stream failed; polling").entered();
                source.close();
                poll(Rc::clone(&on_versions));
            }
        });
        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    }
}
