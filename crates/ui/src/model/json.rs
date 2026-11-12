//! `JSON.parse` into a value that keeps JavaScript key order, and
//! `JSON.stringify(v, null, 2)` back out. Object keys follow
//! `Object.entries`: array-index keys first in ascending order, then the
//! rest in the order they first appear; a repeated key keeps its first place
//! and its last value.

use std::fmt::{self, Write as _};

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use super::jsnum::to_json;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `typeof v === "object" && v !== null`: arrays and objects.
    pub const fn is_object(&self) -> bool {
        matches!(self, Self::Arr(_) | Self::Obj(_))
    }
}

/// A canonical array index: `0` or no leading zero, below 2^32 - 1.
fn index_key(k: &str) -> Option<u32> {
    let ok =
        !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) && (k == "0" || !k.starts_with('0'));
    ok.then(|| k.parse::<u32>().ok())
        .flatten()
        .filter(|n| *n < u32::MAX)
}

fn order_keys(mut entries: Vec<(String, Json)>) -> Vec<(String, Json)> {
    if entries.iter().any(|(k, _)| index_key(k).is_some()) {
        // Stable: non-index keys keep their order.
        entries.sort_by_key(|(k, _)| index_key(k).map_or((1, 0), |n| (0, n)));
    }
    entries
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON")
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
        Ok(Json::Bool(v))
    }

    #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
    fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
        Ok(Json::Num(v as f64))
    }

    #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
    fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
        Ok(Json::Num(v as f64))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
        Ok(Json::Num(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Json, E> {
        Ok(Json::Str(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Json, E> {
        Ok(Json::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element()? {
            out.push(v);
        }
        Ok(Json::Arr(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut out: Vec<(String, Json)> = Vec::new();
        while let Some((k, v)) = map.next_entry::<String, Json>()? {
            match out.iter_mut().find(|(have, _)| *have == k) {
                Some(slot) => slot.1 = v,
                None => out.push((k, v)),
            }
        }
        Ok(Json::Obj(order_keys(out)))
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(JsonVisitor)
    }
}

/// `JSON.parse(s)`, or `None` where it would throw. Lone surrogate escapes
/// read as U+FFFD.
pub fn parse(s: &str) -> Option<Json> {
    serde_json::from_str(&peektrace_protocol::view::mend_surrogates(s)).ok()
}

fn push_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_pretty(out: &mut String, v: &Json, depth: usize) {
    let pad = |out: &mut String, d: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', d * 2));
    };
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => out.push_str(&to_json(*n)),
        Json::Str(s) => push_quoted(out, s),
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(entries) if entries.is_empty() => out.push_str("{}"),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, depth + 1);
                write_pretty(out, item, depth + 1);
            }
            pad(out, depth);
            out.push(']');
        }
        Json::Obj(entries) => {
            out.push('{');
            for (i, (k, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, depth + 1);
                push_quoted(out, k);
                out.push_str(": ");
                write_pretty(out, item, depth + 1);
            }
            pad(out, depth);
            out.push('}');
        }
    }
}

/// `JSON.stringify(v, null, 2)`.
pub fn pretty(v: &Json) -> String {
    let mut out = String::new();
    write_pretty(&mut out, v, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_js_key_order() {
        let v = parse(r#"{"b":1,"10":2,"a":[],"2":{},"b":3,"01":4}"#).unwrap_or(Json::Null);
        assert_eq!(
            pretty(&v),
            "{\n  \"2\": {},\n  \"10\": 2,\n  \"b\": 3,\n  \"a\": [],\n  \"01\": 4\n}"
        );
    }

    #[test]
    fn prints_like_stringify() {
        let v = parse(r#"[1.50, -0, 1e21, "tab\there", null, true]"#).unwrap_or(Json::Null);
        assert_eq!(
            pretty(&v),
            "[\n  1.5,\n  0,\n  1e+21,\n  \"tab\\there\",\n  null,\n  true\n]"
        );
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(" 42 "), Some(Json::Num(42.0)));
    }
}
