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

// settings-service.test.ts, same inputs and assertions.

#[test]
fn update_writes_pretty_json_and_get_reads_it_back() {
    let path = dir("pretty").join("settings.json");
    let p = path.to_string_lossy().into_owned();
    let d = doc(r#"{"roots":{"claude":[{"path":"/work/.claude","label":"work"}]}}"#);
    let written = write_stored(&p, d.clone(), None).expect("write");
    assert!(written.mtime_ms > 0);
    let raw = std::fs::read_to_string(&path).expect("read");
    assert_eq!(
        JsValue::parse(&raw).expect("json").stringify(false),
        r#"{"roots":{"claude":[{"path":"/work/.claude","label":"work"}]}}"#
    );
    let reread = read_stored(&p);
    assert_eq!(reread.doc, d);
    assert_eq!(reread.mtime_ms, written.mtime_ms);
}

#[test]
fn a_stale_expected_mtime_fails_and_does_not_clobber_the_file() {
    let path = dir("clobber").join("settings.json");
    let p = path.to_string_lossy().into_owned();
    write_stored(&p, doc(r#"{"roots":{"codex":[{"path":"/a"}]}}"#), None).expect("write");
    let stale = write_stored(
        &p,
        doc(r#"{"roots":{"codex":[{"path":"/b"}]}}"#),
        Some(12_345.0),
    );
    assert!(
        matches!(stale, Err(SettingsUpdateError::Changed { .. })),
        "{stale:?}"
    );
    let raw = std::fs::read_to_string(&path).expect("read");
    assert_eq!(
        JsValue::parse(&raw).expect("json").stringify(false),
        r#"{"roots":{"codex":[{"path":"/a"}]}}"#
    );
}

#[test]
fn expected_mtime_zero_succeeds_when_the_file_is_absent() {
    let path = dir("zero").join("settings.json");
    let p = path.to_string_lossy().into_owned();
    let first = write_stored(&p, SettingsDoc::default(), Some(0.0)).expect("write");
    assert!(first.path.ends_with("settings.json"));
    assert!(std::path::Path::new(&first.path).exists());
}
