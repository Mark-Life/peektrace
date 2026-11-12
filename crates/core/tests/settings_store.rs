//! Settings file reads and compare-and-swap writes
//! (`settings-service.test.ts`).

use std::path::PathBuf;

use peektrace_core::js::value::JsValue;
use peektrace_core::settings_doc::{
    ChangeReason, SettingsDoc, SettingsUpdateError, read_stored, validate_settings, write_stored,
};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("peektrace-settings-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn doc(json: &str) -> SettingsDoc {
    validate_settings(&JsValue::parse(json).expect("json")).expect("valid")
}

#[test]
fn missing_file_reads_empty() {
    let path = dir("missing").join("settings.json");
    let s = read_stored(&path.to_string_lossy());
    assert_eq!(s.doc, SettingsDoc::default());
    assert_eq!(s.mtime_ms, 0);
}

#[test]
fn write_creates_the_dir_and_round_trips() {
    let path = dir("roundtrip").join("nested").join("settings.json");
    let p = path.to_string_lossy().into_owned();
    let d = doc(r#"{"roots":{"claude":[{"path":"~/work/.claude","label":"work"}]}}"#);
    let written = write_stored(&p, d.clone(), Some(0.0)).expect("write");
    assert!(written.mtime_ms > 0);
    let read = read_stored(&p);
    assert_eq!(read.doc, d);
    assert_eq!(read.mtime_ms, written.mtime_ms);
    let leftovers = std::fs::read_dir(path.parent().expect("dir"))
        .expect("ls")
        .count();
    assert_eq!(leftovers, 1);
}

#[test]
fn stale_mtime_is_refused() {
    let path = dir("stale").join("settings.json");
    let p = path.to_string_lossy().into_owned();
    let first = write_stored(&p, SettingsDoc::default(), None).expect("write");
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64")]
    let stale = (first.mtime_ms - 1) as f64;
    match write_stored(&p, SettingsDoc::default(), Some(stale)) {
        Err(SettingsUpdateError::Changed { reason, .. }) => assert_eq!(reason, ChangeReason::Mtime),
        other => panic!("expected a CAS failure, got {other:?}"),
    }
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64")]
    let current = first.mtime_ms as f64;
    assert!(write_stored(&p, SettingsDoc::default(), Some(current)).is_ok());
}

#[test]
fn missing_file_with_an_mtime_is_refused() {
    let path = dir("gone").join("settings.json");
    let p = path.to_string_lossy().into_owned();
    match write_stored(&p, SettingsDoc::default(), Some(5.0)) {
        Err(SettingsUpdateError::Changed { reason, .. }) => {
            assert_eq!(reason, ChangeReason::Missing);
        }
        other => panic!("expected a CAS failure, got {other:?}"),
    }
}
