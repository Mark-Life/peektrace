//! The wide event: one per root span, stored as JSON in the TS schema
//! (`packages/telemetry/src/schema.ts`), keys in schema order.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher as _;

use peektrace_core::js::json_out::push_str;
use peektrace_core::js::num;
use peektrace_core::js::value::{JsObject, JsValue};

/// A success slower than this is flagged interesting.
pub const SLOW_MS: f64 = 2000.0;

/// The kinds the TS schema accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cli,
    Rpc,
}

impl Kind {
    /// The root's `peektrace.kind`: absent is `cli`; any other value is
    /// stored as `rpc`, the only other kind the TS reader accepts.
    pub fn of(raw: Option<&str>) -> Self {
        match raw {
            None | Some("cli") => Self::Cli,
            Some(_) => Self::Rpc,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Rpc => "rpc",
        }
    }
}

/// How the root span ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Error { tag: String, message: String },
    Defect { message: String },
}

impl Outcome {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error { .. } => "error",
            Self::Defect { .. } => "defect",
        }
    }

    pub fn error_tag(&self) -> Option<&str> {
        match self {
            Self::Error { tag, .. } => Some(tag),
            _ => None,
        }
    }
}

/// A finished child span; `attributes` is a JSON object.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanRecord {
    pub name: String,
    pub duration_ms: f64,
    pub attributes: String,
}

/// One wide event. `attributes` is a JSON object.
#[derive(Debug, Clone, PartialEq)]
pub struct WideEvent {
    pub id: String,
    pub trace_id: String,
    pub ts: i64,
    pub kind: Kind,
    pub name: String,
    pub app_version: String,
    pub platform: String,
    pub duration_ms: f64,
    pub attributes: String,
    pub spans: Vec<SpanRecord>,
    pub outcome: Outcome,
}

/// Flag, not a drop filter: local keeps every event.
pub fn is_interesting(outcome: &Outcome, duration_ms: f64) -> bool {
    *outcome != Outcome::Success || duration_ms > SLOW_MS
}

fn push_key(out: &mut String, key: &str) {
    push_str(out, key);
    out.push(':');
}

impl WideEvent {
    pub fn interesting(&self) -> bool {
        is_interesting(&self.outcome, self.duration_ms)
    }

    /// `JSON.stringify(event)`.
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(256 + self.attributes.len());
        out.push('{');
        let text = |out: &mut String, key: &str, value: &str| {
            push_key(out, key);
            push_str(out, value);
            out.push(',');
        };
        text(&mut out, "id", &self.id);
        text(&mut out, "traceId", &self.trace_id);
        push_key(&mut out, "ts");
        out.push_str(&self.ts.to_string());
        out.push(',');
        text(&mut out, "kind", self.kind.as_str());
        text(&mut out, "name", &self.name);
        text(&mut out, "appVersion", &self.app_version);
        text(&mut out, "platform", &self.platform);
        push_key(&mut out, "durationMs");
        out.push_str(&num::to_json(self.duration_ms));
        out.push(',');
        push_key(&mut out, "attributes");
        out.push_str(&self.attributes);
        out.push(',');
        push_key(&mut out, "spans");
        out.push('[');
        for (i, s) in self.spans.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            text(&mut out, "name", &s.name);
            push_key(&mut out, "durationMs");
            out.push_str(&num::to_json(s.duration_ms));
            out.push(',');
            push_key(&mut out, "attributes");
            out.push_str(&s.attributes);
            out.push('}');
        }
        out.push_str("],");
        push_key(&mut out, "outcome");
        push_str(&mut out, self.outcome.as_str());
        match &self.outcome {
            Outcome::Success => {}
            Outcome::Error { tag, message } => {
                out.push(',');
                push_key(&mut out, "error");
                out.push('{');
                text(&mut out, "tag", tag);
                push_key(&mut out, "message");
                push_str(&mut out, message);
                out.push_str(",\"fields\":{}}");
            }
            Outcome::Defect { message } => {
                out.push(',');
                push_key(&mut out, "error");
                out.push('{');
                push_key(&mut out, "message");
                push_str(&mut out, message);
                out.push('}');
            }
        }
        out.push('}');
        out
    }
}

/// `<os>; rust/<arch>`, with the OS named as Node's `process.platform`.
pub fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    format!("{os}; rust/{}", std::env::consts::ARCH)
}

/// A random UUID v4, lowercase.
pub fn uuid_v4() -> String {
    let hi = RandomState::new().hash_one(0u8);
    let lo = RandomState::new().hash_one(1u8);
    let n = (u128::from(hi) << 64) | u128::from(lo);
    let n = (n & !(0xF << 76) & !(0x3 << 62)) | (0x4 << 76) | (0x2 << 62);
    let h = format!("{n:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// A per-process id as TS prints it: hex, 16 wide.
pub fn hex_id(n: u64) -> String {
    format!("{n:016x}")
}

/// A type check on one schema field.
type Check = fn(&JsValue) -> bool;

fn pick(o: &JsObject, key: &str, want: Check) -> Option<(String, JsValue)> {
    o.get(key)
        .filter(|v| want(v))
        .map(|v| (key.to_owned(), v.clone()))
}

fn is_str(v: &JsValue) -> bool {
    matches!(v, JsValue::Str(_))
}

fn is_num(v: &JsValue) -> bool {
    matches!(v, JsValue::Num(_))
}

fn is_record(v: &JsValue) -> bool {
    matches!(v, JsValue::Obj(_))
}

/// Keep `keys` of `o` in this order, each passing its check; `None` when one
/// is missing or of the wrong type.
fn shape(o: &JsObject, keys: &[(&str, Check)]) -> Option<Vec<(String, JsValue)>> {
    keys.iter().map(|(k, want)| pick(o, k, *want)).collect()
}

fn span_record(v: &JsValue) -> Option<JsValue> {
    let o = v.as_obj()?;
    let kept = shape(
        o,
        &[
            ("name", is_str),
            ("durationMs", is_num),
            ("attributes", is_record),
        ],
    )?;
    Some(JsValue::Obj(JsObject::from_entries(kept)))
}

fn error_info(outcome: &str, v: &JsValue) -> Option<JsValue> {
    let o = v.as_obj()?;
    let mut kept = match outcome {
        "error" => shape(o, &[("tag", is_str), ("message", is_str)])?,
        _ => shape(o, &[("message", is_str)])?,
    };
    if outcome == "error"
        && let Some(fields) = o.get("fields")
    {
        if !is_record(fields) {
            return None;
        }
        kept.push(("fields".to_owned(), fields.clone()));
    }
    Some(JsValue::Obj(JsObject::from_entries(kept)))
}

/// A stored event as the TS schema decodes it: unknown keys dropped at
/// every level, keys in schema order. `None` when the TS decoder rejects it.
pub fn decode(data: &str) -> Option<JsValue> {
    let v = JsValue::parse(data)?;
    let o = v.as_obj()?;
    let mut kept = shape(
        o,
        &[
            ("id", is_str),
            ("traceId", is_str),
            ("ts", is_num),
            ("kind", |v| matches!(v.as_str(), Some("cli" | "rpc"))),
            ("name", is_str),
            ("appVersion", is_str),
            ("platform", is_str),
            ("durationMs", is_num),
            ("attributes", is_record),
        ],
    )?;
    let spans = o
        .get("spans")?
        .as_arr()?
        .iter()
        .map(span_record)
        .collect::<Option<Vec<_>>>()?;
    kept.push(("spans".to_owned(), JsValue::Arr(spans)));
    let outcome = o.get("outcome")?.as_str()?;
    if !matches!(outcome, "success" | "error" | "defect") {
        return None;
    }
    kept.push(("outcome".to_owned(), JsValue::Str(outcome.to_owned())));
    if outcome != "success" {
        let error = error_info(outcome, o.get("error")?)?;
        kept.push(("error".to_owned(), error));
    }
    Some(JsValue::Obj(JsObject::from_entries(kept)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(outcome: Outcome, duration_ms: f64) -> WideEvent {
        WideEvent {
            id: "1".to_owned(),
            trace_id: "t".to_owned(),
            ts: 0,
            kind: Kind::Cli,
            name: "n".to_owned(),
            app_version: "v".to_owned(),
            platform: "p".to_owned(),
            duration_ms,
            attributes: "{}".to_owned(),
            spans: Vec::new(),
            outcome,
        }
    }

    #[test]
    fn interesting_rules() {
        assert!(!event(Outcome::Success, 10.0).interesting());
        assert!(!event(Outcome::Success, SLOW_MS).interesting());
        assert!(event(Outcome::Success, SLOW_MS + 1.0).interesting());
        let error = Outcome::Error {
            tag: "E".to_owned(),
            message: "m".to_owned(),
        };
        assert!(event(error, 10.0).interesting());
        let defect = Outcome::Defect {
            message: "m".to_owned(),
        };
        assert!(event(defect, 10.0).interesting());
    }

    #[test]
    fn json_in_schema_order() {
        let mut e = event(
            Outcome::Error {
                tag: "Error".to_owned(),
                message: "no \"x\"".to_owned(),
            },
            1.5,
        );
        e.attributes = r#"{"argv":"sessions ls"}"#.to_owned();
        e.spans.push(SpanRecord {
            name: "s".to_owned(),
            duration_ms: 0.25,
            attributes: "{}".to_owned(),
        });
        assert_eq!(
            e.to_json(),
            r#"{"id":"1","traceId":"t","ts":0,"kind":"cli","name":"n","appVersion":"v","platform":"p","durationMs":1.5,"attributes":{"argv":"sessions ls"},"spans":[{"name":"s","durationMs":0.25,"attributes":{}}],"outcome":"error","error":{"tag":"Error","message":"no \"x\"","fields":{}}}"#
        );
        let back = decode(&e.to_json()).map(|v| v.stringify(false));
        assert_eq!(back.as_deref(), Some(e.to_json().as_str()));
    }

    #[test]
    fn decode_drops_unknown_keys_and_orders() {
        let src = r#"{"zz":1,"traceId":"t","id":"i","ts":5,"kind":"rpc","name":"n","appVersion":"v","platform":"p","durationMs":1,"attributes":{"b":1,"a":[2]},"spans":[{"x":1,"attributes":{},"durationMs":2,"name":"c"}],"outcome":"success","error":{"message":"m"}}"#;
        assert_eq!(
            decode(src).map(|v| v.stringify(false)).as_deref(),
            Some(
                r#"{"id":"i","traceId":"t","ts":5,"kind":"rpc","name":"n","appVersion":"v","platform":"p","durationMs":1,"attributes":{"b":1,"a":[2]},"spans":[{"name":"c","durationMs":2,"attributes":{}}],"outcome":"success"}"#
            )
        );
        let defect = src
            .replace(r#""success""#, r#""defect""#)
            .replace(r#"{"message":"m"}"#, r#"{"tag":"T","message":"m"}"#);
        assert!(
            decode(&defect)
                .map(|v| v.stringify(false))
                .is_some_and(|s| s.ends_with(r#""outcome":"defect","error":{"message":"m"}}"#))
        );
        assert_eq!(decode(&src.replace(r#""rpc""#, r#""tui""#)), None);
        assert_eq!(decode(&src.replace(r#""success""#, r#""error""#)), None);
        assert_eq!(decode("{"), None);
    }

    #[test]
    fn ids() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(uuid_v4(), id);
        assert_eq!(hex_id(1), "0000000000000001");
        assert!(platform().contains("; rust/"));
    }
}
