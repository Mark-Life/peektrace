//! One `OpenCode` session as the reader loads it: the session row, then each
//! message with its parts, hydrated as JSON objects. Parts print whole in
//! event bodies, so they keep every key the reader adds (`id`, `sessionID`,
//! `messageID`, `kind`).

use crate::js::json_out::Writer;
use crate::js::text;
use crate::js::value::{JsObject, JsValue};

/// The session row, or the legacy tree's session file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    pub time_created: f64,
    pub time_updated: f64,
    pub cost: f64,
    pub tokens_input: f64,
    pub tokens_output: f64,
    pub tokens_reasoning: f64,
    pub tokens_cache_read: f64,
    pub tokens_cache_write: f64,
    pub project_id: Option<String>,
    pub directory: Option<String>,
    pub title: Option<String>,
    pub version: Option<String>,
    /// A parsed JSON object, the raw column text, or (tree) any JSON value.
    pub model: Option<JsValue>,
    pub agent: Option<String>,
}

/// A hydrated message and its parts, in load order.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub data: JsObject,
    pub parts: Vec<JsObject>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub info: SessionInfo,
    pub messages: Vec<Message>,
}

/// A column or JSON field value as `bun:sqlite` hands it over.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Num(f64),
    Text(String),
    /// A BLOB: neither a string nor a number.
    Other,
}

impl Cell {
    /// `typeof v === "number" ? v : 0`.
    pub fn num(&self) -> f64 {
        match self {
            Self::Num(n) => *n,
            _ => 0.0,
        }
    }

    /// A non-empty string.
    pub fn opt_str(&self) -> Option<String> {
        match self {
            Self::Text(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn to_js(&self) -> JsValue {
        match self {
            Self::Num(n) => JsValue::Num(*n),
            Self::Text(s) => JsValue::Str(s.clone()),
            Self::Null | Self::Other => JsValue::Null,
        }
    }
}

/// Column text as transcript text.
pub fn text_cell(s: &str) -> Cell {
    Cell::Text(text::escape_reserved(s).into_owned())
}

/// A JSON text that parses to a plain object.
pub fn parse_object(raw: &str) -> Option<JsObject> {
    match JsValue::parse(raw)? {
        JsValue::Obj(o) => Some(o),
        _ => None,
    }
}

/// An object value after the reader's JSON round trip.
fn round_trip(o: JsObject) -> JsObject {
    match JsValue::Obj(o).round_trip() {
        JsValue::Obj(o) => o,
        _ => JsObject::default(),
    }
}

/// `{...data, time?, id, sessionID, kind: "message"}`: `time.created` comes
/// from the column only when the JSON has no number there.
pub fn hydrate_message(
    data: Option<JsObject>,
    id: &str,
    session_id: &str,
    created: f64,
) -> JsObject {
    let mut data = data.unwrap_or_default();
    let time = data.get("time").and_then(JsValue::as_obj);
    if !time.is_some_and(|t| matches!(t.get("created"), Some(JsValue::Num(_)))) {
        let mut time = time.cloned().unwrap_or_default();
        time.set("created", JsValue::Num(created));
        data.set("time", JsValue::Obj(time));
    }
    finish_message(data, id, session_id)
}

/// A tree message: `{...data, id, sessionID, kind: "message"}`.
pub fn finish_message(mut data: JsObject, id: &str, session_id: &str) -> JsObject {
    data.set("id", JsValue::Str(id.to_owned()));
    data.set("sessionID", JsValue::Str(session_id.to_owned()));
    data.set("kind", JsValue::Str("message".to_owned()));
    round_trip(data)
}

/// `{...data, id, sessionID, messageID, kind: "part"}`.
pub fn hydrate_part(
    data: Option<JsObject>,
    id: JsValue,
    session_id: &str,
    message_id: &str,
) -> JsObject {
    let mut data = data.unwrap_or_default();
    data.set("id", id);
    data.set("sessionID", JsValue::Str(session_id.to_owned()));
    data.set("messageID", JsValue::Str(message_id.to_owned()));
    data.set("kind", JsValue::Str("part".to_owned()));
    round_trip(data)
}

/// `JSON.stringify(o)`.
fn obj_json(o: &JsObject) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    o.entries().for_each(|(k, v)| v.write(w.key_text(k)));
    w.end_obj();
    w.finish()
}

/// The dialect's leading `session` line.
fn session_line(s: &SessionInfo) -> JsValue {
    let str_val = |v: &str| JsValue::Str(v.to_owned());
    let fixed = [
        ("kind", str_val("session")),
        ("id", str_val(&s.id)),
        ("timeCreated", JsValue::Num(s.time_created)),
        ("timeUpdated", JsValue::Num(s.time_updated)),
        ("cost", JsValue::Num(s.cost)),
        ("tokensInput", JsValue::Num(s.tokens_input)),
        ("tokensOutput", JsValue::Num(s.tokens_output)),
        ("tokensReasoning", JsValue::Num(s.tokens_reasoning)),
        ("tokensCacheRead", JsValue::Num(s.tokens_cache_read)),
        ("tokensCacheWrite", JsValue::Num(s.tokens_cache_write)),
    ];
    let optional = [
        ("projectId", s.project_id.as_deref().map(str_val)),
        ("directory", s.directory.as_deref().map(str_val)),
        ("title", s.title.as_deref().map(str_val)),
        ("version", s.version.as_deref().map(str_val)),
        ("model", s.model.clone()),
        ("agent", s.agent.as_deref().map(str_val)),
    ];
    let entries = fixed.into_iter().map(|(k, v)| (k.to_owned(), v)).chain(
        optional
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k.to_owned(), v))),
    );
    JsValue::Obj(JsObject::from_entries(entries))
}

impl Session {
    /// UTF-8 bytes of the JSONL text the TS reader builds for this session:
    /// the session line, then each message line followed by its parts.
    pub fn dialect_len(&self) -> u64 {
        let (lines, bytes) = self.messages.iter().fold(
            (1_usize, session_line(&self.info).json_len()),
            |(n, b), m| {
                let parts: usize = m.parts.iter().map(JsObject::json_len).sum();
                (n + 1 + m.parts.len(), b + m.data.json_len() + parts)
            },
        );
        u64::try_from(bytes + lines - 1).unwrap_or(u64::MAX)
    }

    /// The JSONL text the TS reader builds for this session (see
    /// [`Self::dialect_len`]), joined by `\n` with no trailing newline.
    pub fn dialect_text(&self) -> String {
        let mut out = String::with_capacity(usize::try_from(self.dialect_len()).unwrap_or(0));
        out.push_str(&session_line(&self.info).stringify(false));
        for m in &self.messages {
            out.push('\n');
            out.push_str(&obj_json(&m.data));
            for p in &m.parts {
                out.push('\n');
                out.push_str(&obj_json(p));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_data_keeps_its_fields() {
        let deep = format!("{}1{}", r#"{"d":"#.repeat(200), "}".repeat(200));
        let raw = format!(r#"{{"role":"assistant","modelID":"m","deep":{deep}}}"#);
        let o = parse_object(&raw);
        assert_eq!(
            o.as_ref()
                .and_then(|o| o.get("role"))
                .and_then(JsValue::as_str),
            Some("assistant")
        );
        assert_eq!(o.map(|o| o.json_len()), Some(raw.len()));
    }
}
