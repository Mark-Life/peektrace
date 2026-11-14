//! The core settings service through the in-process source, on temp homes:
//! the absent file, the round trip, the compare-and-swap, bad entries and
//! read-only mode. Never the real `~/.peektrace`.

mod common;

use std::path::{Path, PathBuf};

use common::home;
use peektrace_protocol::{ApiError, READ_ONLY_REASON, SettingsUpdate};
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::source::{DataSource as _, LoadError};

fn settings_file(home: &Path) -> PathBuf {
    home.join(".peektrace/settings.json")
}

fn source(home: &Path) -> InProcess {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    InProcess::for_home(dir, move |k| {
        (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
    })
}

fn update(settings: serde_json::Value, expected: Option<f64>) -> SettingsUpdate {
    SettingsUpdate {
        settings: serde_json::from_value(settings).unwrap_or_default(),
        expected_mtime: expected,
    }
}

#[test]
fn an_absent_file_reads_empty_with_mtime_zero() {
    let dir = home::temp_home("svc-absent");
    let got = source(&dir).settings_now().and_then(Result::ok);
    let got = got.expect("settings");
    assert_eq!(
        serde_json::to_string(&got.settings).ok().as_deref(),
        Some("{}")
    );
    assert_eq!(got.mtime_ms, 0);
    assert_eq!(got.path, settings_file(&dir).to_string_lossy());
}

#[test]
fn a_write_round_trips_in_agent_order() {
    let dir = home::temp_home("svc-round");
    let src = source(&dir);
    let saved = src
        .save_settings_now(update(
            serde_json::json!({"roots": {"pi": [{"path": "~/p", "label": "x"}], "claude": [{"path": "~/c"}]}}),
            Some(0.0),
        ))
        .and_then(Result::ok)
        .expect("saved");
    assert!(saved.mtime_ms > 0);
    let text = std::fs::read_to_string(settings_file(&dir)).unwrap_or_default();
    assert!(text.find("claude") < text.find("\"pi\""), "{text}");
    let back = src.settings_now().and_then(Result::ok).expect("read");
    assert_eq!(back.settings, saved.settings);
    assert_eq!(back.mtime_ms, saved.mtime_ms);
}

#[test]
fn a_stale_mtime_is_a_file_changed_error() {
    let dir = home::temp_home("svc-stale");
    let src = source(&dir);
    let first = src
        .save_settings_now(update(serde_json::json!({}), None))
        .and_then(Result::ok)
        .expect("first");
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    let stale = (first.mtime_ms - 1000) as f64;
    let got = src.save_settings_now(update(serde_json::json!({}), Some(stale)));
    assert!(matches!(
        got,
        Some(Err(LoadError::Api(ApiError::FileChangedError {
            reason: peektrace_protocol::FileChangeReason::Mtime,
            ..
        })))
    ));
    assert_eq!(
        std::fs::read_to_string(settings_file(&dir)).ok().as_deref(),
        Some("{}\n")
    );
}

#[test]
fn expected_zero_passes_only_while_the_file_is_absent() {
    let dir = home::temp_home("svc-zero");
    let src = source(&dir);
    assert!(matches!(
        src.save_settings_now(update(serde_json::json!({}), Some(0.0))),
        Some(Ok(_))
    ));
    assert!(matches!(
        src.save_settings_now(update(serde_json::json!({}), Some(0.0))),
        Some(Err(LoadError::Api(ApiError::FileChangedError { .. })))
    ));
    std::fs::remove_file(settings_file(&dir)).expect("rm");
    assert!(matches!(
        src.save_settings_now(update(serde_json::json!({}), Some(5.0))),
        Some(Err(LoadError::Api(ApiError::FileChangedError {
            reason: peektrace_protocol::FileChangeReason::Missing,
            ..
        })))
    ));
}

#[test]
fn bad_entries_and_read_only_are_refused() {
    let dir = home::temp_home("svc-bad");
    let src = source(&dir);
    let bad = src.save_settings_now(update(
        serde_json::json!({"roots": {"claude": [{"label": "x"}]}}),
        None,
    ));
    assert!(matches!(
        bad,
        Some(Err(LoadError::Api(ApiError::BadRequestError { reason })))
            if reason == "settings.roots entries must be arrays of {path: string, label?: string}"
    ));
    let locked = source(&dir).with_read_only(true);
    let got = locked.save_settings_now(update(serde_json::json!({}), None));
    assert!(matches!(
        got,
        Some(Err(LoadError::Api(ApiError::SettingsWriteError { ref path, ref reason })))
            if path == "settings.json" && reason == READ_ONLY_REASON
    ));
    assert!(!settings_file(&dir).exists());
    assert!(
        locked
            .server_info_now()
            .and_then(Result::ok)
            .is_some_and(|i| i.read_only)
    );
}
