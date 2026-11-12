//! `peektrace doctor`: write a redacted bundle of recent telemetry events
//! for support. Nothing leaves the machine until the user emails the file.

use std::io;
use std::path::Path;
use std::sync::LazyLock;

use peektrace_core::PathStyle;
use peektrace_core::js::num;
use peektrace_core::js::value::{JsObject, JsValue};
use peektrace_core::sessions::redact::redact_text;
use peektrace_telemetry::{Store, event, store};
use regex::Regex;

use crate::args::DoctorArgs;
use crate::ls::{UserError, parse_limit};

const SCHEMA: &str = "peektrace-report/v1";
const UNREADABLE_ATTR: &str = "sessions.unreadable";
const MIN_ENTROPY: f64 = 3.5;
const MARK: &str = "[REDACTED:high-entropy]";

/// Keys whose values may hold a bare secret no provider rule knows (the
/// `argv` command line, a future token attribute).
static CRED_KEY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)key|token|secret|password|passwd|pwd|auth|cred|argv").ok());
static HIGH_ENTROPY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/_-]{20,}={0,2}").ok());

/// Shannon entropy in bits per char, summed in first-seen order as TS does.
fn entropy(s: &str) -> f64 {
    let mut freq: Vec<(char, u32)> = Vec::new();
    for c in s.chars() {
        match freq.iter_mut().find(|(k, _)| *k == c) {
            Some(slot) => slot.1 += 1,
            None => freq.push((c, 1)),
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "token lengths are small")]
    let len = s.chars().count() as f64;
    freq.iter().fold(0.0, |h, (_, n)| {
        let p = f64::from(*n) / len;
        h - (p * p.ln()) / 2f64.ln()
    })
}

/// Replace every bare high-entropy token: a run of 20+ base64-ish chars
/// with a digit and a letter, not already a redaction mark.
fn redact_high_entropy(s: &str) -> String {
    let Some(re) = HIGH_ENTROPY.as_ref() else {
        return s.to_owned();
    };
    re.replace_all(s, |c: &regex::Captures<'_>| {
        let m = &c[0];
        let secret = m.bytes().any(|b| b.is_ascii_digit())
            && m.bytes().any(|b| b.is_ascii_alphabetic())
            && !m.starts_with("[REDACTED")
            && entropy(m) >= MIN_ENTROPY;
        if secret {
            MARK.to_owned()
        } else {
            m.to_owned()
        }
    })
    .into_owned()
}

fn credential_key(key: Option<&str>) -> bool {
    key.is_some_and(|k| CRED_KEY.as_ref().is_some_and(|re| re.is_match(k)))
}

/// Every string redacted; values under a credential-like key also lose
/// bare high-entropy tokens. Array items take the array's key.
pub fn redact_json(v: &JsValue, key: Option<&str>) -> JsValue {
    match v {
        JsValue::Str(s) => {
            let base = redact_text(s);
            JsValue::Str(if credential_key(key) {
                redact_high_entropy(&base)
            } else {
                base.into_owned()
            })
        }
        JsValue::Arr(items) => JsValue::Arr(items.iter().map(|i| redact_json(i, key)).collect()),
        JsValue::Obj(o) => JsValue::Obj(JsObject::from_entries(
            o.entries()
                .map(|(k, val)| (k.to_owned(), redact_json(val, Some(k)))),
        )),
        other => other.clone(),
    }
}

/// The unreadable count of the newest event (events newest first) that
/// ran a listing; 0 when that count is not a number or none did.
pub fn last_unreadable_count(events: &[JsValue]) -> f64 {
    events
        .iter()
        .find_map(|e| e.get("attributes").and_then(|a| a.get(UNREADABLE_ATTR)))
        .and_then(JsValue::as_num)
        .unwrap_or(0.0)
}

/// The bundle text and the count of events in it.
pub fn bundle(events: &[JsValue], generated_at: f64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "row counts fit f64")]
    let count = events.len() as f64;
    let o = JsObject::from_entries([
        ("schema".to_owned(), JsValue::Str(SCHEMA.to_owned())),
        ("generatedAt".to_owned(), JsValue::Num(generated_at)),
        ("count".to_owned(), JsValue::Num(count)),
        (
            "events".to_owned(),
            JsValue::Arr(events.iter().map(|e| redact_json(e, None)).collect()),
        ),
    ]);
    JsValue::Obj(o).stringify(true)
}

/// What TS prints for a failed `FileSystem.writeFile`.
pub fn write_error(path: &str, e: &io::Error) -> String {
    if path.is_empty() {
        return "NotFound: FileSystem.writeFile (): ENOENT: no such file or directory, open"
            .to_owned();
    }
    let (tag, code, text) = match e.kind() {
        io::ErrorKind::NotFound => ("NotFound", "ENOENT", "no such file or directory"),
        io::ErrorKind::IsADirectory => {
            ("BadResource", "EISDIR", "illegal operation on a directory")
        }
        io::ErrorKind::NotADirectory => ("BadResource", "ENOTDIR", "not a directory"),
        io::ErrorKind::PermissionDenied => ("PermissionDenied", "EACCES", "permission denied"),
        _ => return format!("Unknown: FileSystem.writeFile ({path}): {e}"),
    };
    format!("{tag}: FileSystem.writeFile ({path}): {code}: {text}, open '{path}'")
}

/// `<n> session(s) could not be read (last sessions list)`.
fn unreadable_line(n: f64) -> String {
    #[expect(clippy::float_cmp, reason = "TS compares with ===")]
    let noun = if n == 1.0 { "session" } else { "sessions" };
    format!(
        "{} {noun} could not be read (last sessions list)",
        num::to_js_string(n)
    )
}

/// Where the command reads and writes.
pub struct DoctorEnv<'a> {
    /// `PEEKTRACE_DIR` as set, or `None`.
    pub peektrace_dir: Option<&'a str>,
    pub home: &'a str,
    pub now_ms: f64,
}

/// Run `doctor` and return its stdout lines.
pub fn run(args: &DoctorArgs, env: &DoctorEnv<'_>) -> Result<Vec<String>, UserError> {
    let last = parse_limit(&args.last)?;
    let open_failed = |reason: String| {
        UserError::Invalid(format!("Could not open the telemetry store: {reason}"))
    };
    let dir = store::dir_of(env.peektrace_dir, env.home).map_err(|e| open_failed(e.to_string()))?;
    peektrace_telemetry::wait_ready(peektrace_telemetry::EXIT_WAIT);
    #[expect(clippy::cast_possible_truncation, reason = "a safe integer")]
    let (limit, now) = (last as i64, env.now_ms as i64);
    let rows = Store::open(&dir, now)
        .and_then(|s| s.recent(limit, args.interesting_only))
        .map_err(|e| open_failed(e.to_string()))?;
    let events: Vec<JsValue> = rows.iter().filter_map(|r| event::decode(r)).collect();
    let text = bundle(&events, env.now_ms);
    let dir_text = env.peektrace_dir.map_or_else(
        || PathStyle::NATIVE.join(&[env.home, ".peektrace"]),
        str::to_owned,
    );
    let _ = std::fs::create_dir_all(&dir_text);
    let path = args.out.clone().unwrap_or_else(|| {
        PathStyle::NATIVE.join(&[
            &dir_text,
            &format!("peektrace-report-{}.json", events.len()),
        ])
    });
    std::fs::write(Path::new(&path), text)
        .map_err(|e| UserError::Invalid(write_error(&path, &e)))?;
    let unreadable = last_unreadable_count(&events);
    Ok(
        std::iter::once(format!("Wrote {} events → {path}", events.len()))
            .chain((unreadable > 0.0).then(|| unreadable_line(unreadable)))
            .chain(std::iter::once(
                "Email this file to 108@mark-life.com".to_owned(),
            ))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(ts: u32, unreadable: Option<f64>) -> JsValue {
        let attrs = unreadable.map_or_else(JsObject::default, |n| {
            JsObject::from_entries([(UNREADABLE_ATTR.to_owned(), JsValue::Num(n))])
        });
        JsValue::Obj(JsObject::from_entries([
            ("ts".to_owned(), JsValue::Num(f64::from(ts))),
            ("attributes".to_owned(), JsValue::Obj(attrs)),
        ]))
    }

    #[test]
    fn unreadable_from_the_newest_listing() {
        let count = |e: &[JsValue]| last_unreadable_count(e);
        assert_eq!(
            count(&[event(3, None), event(2, Some(2.0)), event(1, Some(5.0))]),
            2.0
        );
        assert_eq!(count(&[event(2, Some(0.0)), event(1, Some(4.0))]), 0.0);
        assert_eq!(count(&[event(1, None)]), 0.0);
        assert_eq!(
            unreadable_line(1.0),
            "1 session could not be read (last sessions list)"
        );
    }

    fn redacted(src: &str) -> String {
        JsValue::parse(src)
            .map(|v| redact_json(&v, None).stringify(false))
            .unwrap_or_default()
    }

    #[test]
    fn sweeps_only_credential_keys() {
        let secret = format!("sk-ant-{}", "A".repeat(50));
        assert_eq!(
            redacted(&format!(r#"{{"argv":"sessions ls","token":"{secret}"}}"#)),
            r#"{"argv":"sessions ls","token":"[REDACTED:anthropic-key]"}"#
        );
        assert_eq!(
            redacted(
                r#"{"argv":"sessions analyze Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1","message":"Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1"}"#
            ),
            r#"{"argv":"sessions analyze [REDACTED:high-entropy]","message":"Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1"}"#
        );
        assert_eq!(
            redacted(
                r#"{"authorization":["Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1",{"x":"Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1"}],"n":[1,null,true]}"#
            ),
            r#"{"authorization":["[REDACTED:high-entropy]",{"x":"Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1"}],"n":[1,null,true]}"#
        );
        assert_eq!(
            redacted(r#"{"argv":"aaaaaaaaaaaaaaaaaaaaaaaaa1 abcdefghijklmnopqrstuvwxyz"}"#),
            r#"{"argv":"aaaaaaaaaaaaaaaaaaaaaaaaa1 abcdefghijklmnopqrstuvwxyz"}"#
        );
    }

    #[test]
    fn bundle_shape() {
        assert_eq!(
            bundle(&[], 5.0),
            "{\n  \"schema\": \"peektrace-report/v1\",\n  \"generatedAt\": 5,\n  \"count\": 0,\n  \"events\": []\n}"
        );
    }

    #[test]
    fn write_errors() {
        let e = |k| io::Error::from(k);
        assert_eq!(
            write_error("missing/x.json", &e(io::ErrorKind::NotFound)),
            "NotFound: FileSystem.writeFile (missing/x.json): ENOENT: no such file or directory, open 'missing/x.json'"
        );
        assert_eq!(
            write_error("d", &e(io::ErrorKind::IsADirectory)),
            "BadResource: FileSystem.writeFile (d): EISDIR: illegal operation on a directory, open 'd'"
        );
        assert_eq!(
            write_error("", &e(io::ErrorKind::NotFound)),
            "NotFound: FileSystem.writeFile (): ENOENT: no such file or directory, open"
        );
    }
}
