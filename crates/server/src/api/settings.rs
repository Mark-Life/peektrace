//! `GET` and `PUT /api/settings`.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::Method;
use peektrace_core::js::value::JsValue;
use peektrace_core::settings_doc;
use peektrace_core::wire;
use peektrace_protocol::ApiError;
use tracing::Instrument as _;

use super::{Res, Shared, bad_request, blocking, finish, json};

pub async fn settings_get(State(state): Shared, method: Method) -> Res {
    let span = rpc_span!("settings.get", method, "/api/settings");
    async move {
        let path = state.settings_path.clone();
        let result = blocking(move || Ok::<_, ApiError>(settings_doc::read_stored(&path))).await;
        finish(result.map(|s| json(wire::settings_body(&s))))
    }
    .instrument(span)
    .await
}

pub async fn settings_put(State(state): Shared, method: Method, body: Bytes) -> Res {
    let span = rpc_span!("settings.update", method, "/api/settings");
    async move {
        let result = async {
            wire::check_settings_writable(state.read_only)?;
            let text = std::str::from_utf8(&body).map_err(|_| bad_request("body is not UTF-8"))?;
            let value = JsValue::parse(text).ok_or_else(|| bad_request("body is not JSON"))?;
            let doc = wire::validated_settings(
                value
                    .get("settings")
                    .ok_or_else(|| bad_request("body needs settings"))?,
            )?;
            let expected = match value.get("expectedMtime") {
                None => None,
                Some(v) => Some(
                    v.as_num()
                        .ok_or_else(|| bad_request("expectedMtime must be a number"))?,
                ),
            };
            let path = state.settings_path.clone();
            let stored = blocking(move || wire::store_settings(&path, doc, expected)).await?;
            Ok(json(wire::settings_body(&stored)))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}
