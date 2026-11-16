//! `GET` and `PUT /api/settings`.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::Method;
use peektrace_core::js::json_out::Writer;
use peektrace_core::js::value::JsValue;
use peektrace_core::settings_doc::{
    self, SettingsUpdateError, StoredSettings, validate_settings, write_settings_doc,
};
use peektrace_protocol::{ApiError, FileChangeReason, READ_ONLY_REASON};
use tracing::Instrument as _;

use super::{Res, Shared, bad_request, blocking, finish, json};

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
        let result = blocking(move || Ok::<_, ApiError>(settings_doc::read_stored(&path))).await;
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
                }
                .into());
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
