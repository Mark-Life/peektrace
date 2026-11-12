//! The `settings.json` document as the settings editor reads and writes it:
//! the decoded roots, the stat mtime used for compare-and-swap, and atomic
//! writes through a temp file.

use std::hash::{BuildHasher as _, RandomState};
use std::path::Path;
use std::time::UNIX_EPOCH;

use tracing::{field, info_span};

use crate::js::json_out::Writer;
use crate::js::text;
use crate::js::value::{EMPTY_OBJECT, JsObject, JsValue};
use crate::model::Agent;

/// One extra root as the document holds it; strings are transcript text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    pub path: String,
    pub label: Option<String>,
}

/// A decoded settings document. `roots` keeps the order it was decoded in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsDoc {
    pub roots: Option<Vec<(Agent, Vec<DocEntry>)>>,
}

/// `RootEntry` decode: `{path: string, label?: string}`, extra keys dropped.
fn doc_entry(v: &JsValue) -> Option<DocEntry> {
    let obj = v.as_obj()?;
    let path = obj.get("path")?.as_str()?.to_owned();
    let label = match obj.get("label") {
        None => None,
        Some(l) => Some(l.as_str()?.to_owned()),
    };
    Some(DocEntry { path, label })
}

/// Every entry of a known agent's array, or `None` when one fails.
fn all_entries(v: &JsValue) -> Option<Vec<DocEntry>> {
    v.as_arr()?.iter().map(doc_entry).collect()
}

/// The schema decode of `roots`: every known agent present must hold only
/// valid entries. Keys come out in agent order.
fn decode_roots(roots: &JsObject) -> Option<Vec<(Agent, Vec<DocEntry>)>> {
    Agent::ALL
        .into_iter()
        .filter_map(|a| roots.get(a.as_str()).map(|v| (a, v)))
        .map(|(a, v)| all_entries(v).map(|e| (a, e)))
        .collect()
}

/// The schema decode of a whole document.
fn decode_doc(v: &JsValue) -> Option<SettingsDoc> {
    let top = match v {
        JsValue::Arr(_) => EMPTY_OBJECT.as_obj(),
        v => v.as_obj(),
    }?;
    match top.get("roots") {
        None => Some(SettingsDoc::default()),
        Some(r) => decode_roots(r.as_obj()?).map(|roots| SettingsDoc { roots: Some(roots) }),
    }
}

/// Keep the entries that decode, per known agent in input order.
fn salvage(v: &JsValue) -> SettingsDoc {
    let Some(roots) = v.get("roots").and_then(JsValue::as_obj) else {
        return SettingsDoc::default();
    };
    let kept: Vec<_> = roots
        .entries()
        .filter_map(|(k, v)| Some((Agent::parse(k)?, v.as_arr()?)))
        .map(|(a, list)| (a, list.iter().filter_map(doc_entry).collect::<Vec<_>>()))
        .filter(|(_, e)| !e.is_empty())
        .collect();
    SettingsDoc {
        roots: (!kept.is_empty()).then_some(kept),
    }
}

/// `parseSettings` as a document: blank or invalid JSON reads as `{}`, a
/// valid file decodes whole, anything else keeps the entries that decode.
pub fn settings_doc(raw: &str) -> SettingsDoc {
    let trimmed = text::trim(raw);
    if trimmed.is_empty() {
        return SettingsDoc::default();
    }
    JsValue::parse(trimmed).map_or_else(SettingsDoc::default, |v| {
        decode_doc(&v).unwrap_or_else(|| salvage(&v))
    })
}

/// Decode a settings value sent by a client, or say why it does not fit.
pub fn validate_settings(v: &JsValue) -> Result<SettingsDoc, String> {
    let obj = v
        .as_obj()
        .ok_or_else(|| "settings must be an object".to_owned())?;
    let Some(roots) = obj.get("roots") else {
        return Ok(SettingsDoc::default());
    };
    let roots = roots
        .as_obj()
        .ok_or_else(|| "settings.roots must be an object".to_owned())?;
    decode_roots(roots)
        .map(|r| SettingsDoc { roots: Some(r) })
        .ok_or_else(|| {
            "settings.roots entries must be arrays of {path: string, label?: string}".to_owned()
        })
}

/// Write the document as a JSON value at the writer's position.
pub fn write_settings_doc(w: &mut Writer, doc: &SettingsDoc) {
    w.begin_obj();
    if let Some(roots) = &doc.roots {
        w.key("roots").begin_obj();
        for (agent, entries) in roots {
            w.key(agent.as_str()).begin_arr();
            for e in entries {
                w.elem().begin_obj();
                w.key("path").text(&e.path);
                if let Some(l) = &e.label {
                    w.key("label").text(l);
                }
                w.end_obj();
            }
            w.end_arr();
        }
        w.end_obj();
    }
    w.end_obj();
}

/// `JSON.stringify(settings, null, 2) + "\n"`.
pub fn settings_file_text(doc: &SettingsDoc) -> String {
    let mut w = Writer::new(true);
    write_settings_doc(&mut w, doc);
    let mut out = w.finish();
    out.push('\n');
    out
}

/// `stat(path).mtime.getTime()`: whole ms, 0 when the file cannot be read.
pub fn mtime_ms(path: &str) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_millis()).ok(),
            Err(e) => i64::try_from(e.duration().as_nanos().div_ceil(1_000_000))
                .ok()
                .map(|ms| -ms),
        })
        .unwrap_or(0)
}

/// The settings file as the editor reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSettings {
    pub doc: SettingsDoc,
    pub mtime_ms: i64,
    pub path: String,
}

/// Stat, then read: the mtime is never newer than the text it labels.
pub fn read_stored(path: &str) -> StoredSettings {
    let mtime_ms = mtime_ms(path);
    let raw = std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    StoredSettings {
        doc: settings_doc(&raw),
        mtime_ms,
        path: path.to_owned(),
    }
}

/// Why the compare-and-swap refused a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeReason {
    Mtime,
    Missing,
}

impl ChangeReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mtime => "mtime",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsUpdateError {
    #[error("{path} changed on disk ({})", reason.as_str())]
    Changed { path: String, reason: ChangeReason },
    #[error("cannot write {path}: {reason}")]
    Write { path: String, reason: String },
}

fn check_cas(path: &str, expected: Option<f64>) -> Result<(), SettingsUpdateError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let changed = |reason| {
        Err(SettingsUpdateError::Changed {
            path: path.to_owned(),
            reason,
        })
    };
    if !Path::new(path).exists() {
        return if expected == 0.0 {
            Ok(())
        } else {
            changed(ChangeReason::Missing)
        };
    }
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    let current = mtime_ms(path) as f64;
    #[expect(clippy::float_cmp, reason = "TS compares with !==")]
    let same = current == expected;
    if same {
        Ok(())
    } else {
        changed(ChangeReason::Mtime)
    }
}

fn temp_token() -> String {
    let n = RandomState::new().hash_one(std::time::SystemTime::now());
    format!("{:012x}", n & 0xffff_ffff_ffff)
}

fn write_atomic(path: &str, content: &str) -> std::io::Result<()> {
    let dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let _ = std::fs::create_dir_all(dir);
    let temp = dir.join(format!(".settings.json.tmp-{}", temp_token()));
    let result = std::fs::write(&temp, content).and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Write `doc` to `path` through a temp file and a rename, after the
/// compare-and-swap on `expected_mtime`. Returns the new state.
pub fn write_stored(
    path: &str,
    doc: SettingsDoc,
    expected_mtime: Option<f64>,
) -> Result<StoredSettings, SettingsUpdateError> {
    let span = info_span!(
        "Settings.update",
        expectedMtime = field::Empty,
        error = field::Empty
    );
    if let Some(m) = expected_mtime {
        span.record("expectedMtime", m);
    }
    let _entered = span.enter();
    let result = check_cas(path, expected_mtime).and_then(|()| {
        write_atomic(path, &settings_file_text(&doc)).map_err(|e| SettingsUpdateError::Write {
            path: path.to_owned(),
            reason: e.to_string(),
        })
    });
    if let Err(e) = &result {
        span.record("error", field::display(e));
    }
    result.map(|()| StoredSettings {
        doc,
        mtime_ms: mtime_ms(path),
        path: path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_json(raw: &str) -> String {
        let mut w = Writer::new(false);
        write_settings_doc(&mut w, &settings_doc(raw));
        w.finish()
    }

    #[test]
    fn blank_and_garbage_read_empty() {
        assert_eq!(doc_json(""), "{}");
        assert_eq!(doc_json("  \n"), "{}");
        assert_eq!(doc_json("{not json"), "{}");
        assert_eq!(doc_json("[]"), "{}");
        assert_eq!(doc_json("3"), "{}");
    }

    #[test]
    fn whole_decode_uses_agent_order() {
        assert_eq!(
            doc_json(
                r#"{"roots":{"codex":[{"path":"~/c","x":1}],"bogus":[1],"claude":[]},"other":1}"#
            ),
            r#"{"roots":{"claude":[],"codex":[{"path":"~/c"}]}}"#
        );
        assert_eq!(doc_json(r#"{"roots":{}}"#), r#"{"roots":{}}"#);
        assert_eq!(
            doc_json(r#"{"roots":{"pi":[{"label":"l","path":"p"}]}}"#),
            r#"{"roots":{"pi":[{"path":"p","label":"l"}]}}"#
        );
    }

    #[test]
    fn salvage_keeps_input_order_and_valid_entries() {
        assert_eq!(
            doc_json(
                r#"{"roots":{"codex":[{"path":"a"},{"path":1}],"claude":[{"path":"b","label":null}],"pi":[{"path":"c"}]}}"#
            ),
            r#"{"roots":{"codex":[{"path":"a"}],"pi":[{"path":"c"}]}}"#
        );
        assert_eq!(doc_json(r#"{"roots":{"claude":[1]}}"#), "{}");
        assert_eq!(doc_json(r#"{"roots":5}"#), "{}");
    }

    #[test]
    fn validation() {
        let ok = |s: &str| validate_settings(&JsValue::parse(s).unwrap());
        assert!(ok("[]").is_err());
        assert!(ok(r#"{"roots":{"claude":[{"path":1}]}}"#).is_err());
        assert!(ok(r#"{"roots":[]}"#).is_err());
        let doc =
            ok(r#"{"roots":{"claude":[{"label":"w","path":"~/x","extra":1}]},"zzz":1}"#).unwrap();
        assert_eq!(
            settings_file_text(&doc),
            "{\n  \"roots\": {\n    \"claude\": [\n      {\n        \"path\": \"~/x\",\n        \"label\": \"w\"\n      }\n    ]\n  }\n}\n"
        );
    }
}
