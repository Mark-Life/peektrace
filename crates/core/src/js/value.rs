//! A parsed JSON value that behaves like the one `JSON.parse` returns: every
//! number is an `f64`, strings are transcript text (see
//! [`super::text::units`]), and object keys keep JS property order: integer
//! index keys first in ascending order, then the rest in insertion order. A
//! repeated key keeps its first position and its last value.

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use super::json::{self, Overflow};
use super::json_out::Writer;
use super::{num, text};

/// One JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsValue>),
    Obj(JsObject),
}

/// A JSON object in JS property order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JsObject(Vec<(String, JsValue)>);

/// The integer value of an array-index key (`0`..`4294967294` in canonical
/// form), which JS orders before other keys.
fn index_key(k: &str) -> Option<u32> {
    let b = k.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b[0] == b'0' {
        return None;
    }
    k.parse::<u64>()
        .ok()
        .filter(|&n| n < 4_294_967_295)
        .and_then(|n| u32::try_from(n).ok())
}

/// `{}`: what `x ?? {}` reads when `x` is missing.
pub static EMPTY_OBJECT: JsValue = JsValue::Obj(JsObject(Vec::new()));

impl JsObject {
    /// `{...self, [key]: value}`: an existing key keeps its place.
    #[must_use]
    pub fn with(mut self, key: &str, value: JsValue) -> Self {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
        Self::from_entries(self.0)
    }

    /// Build from entries in insertion order, applying JS key order.
    pub fn from_entries(entries: impl IntoIterator<Item = (String, JsValue)>) -> Self {
        let mut out: Vec<(String, JsValue)> = Vec::new();
        for (k, v) in entries {
            match out.iter_mut().find(|(have, _)| *have == k) {
                Some(slot) => slot.1 = v,
                None => out.push((k, v)),
            }
        }
        if out.iter().any(|(k, _)| index_key(k).is_some()) {
            out.sort_by_key(|(k, _)| index_key(k).map_or((1, 0), |n| (0, n)));
        }
        Self(out)
    }

    pub fn get(&self, key: &str) -> Option<&JsValue> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &JsValue)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Order `(key, value)` pairs as `Object.fromEntries` would print them.
pub fn js_key_order<T>(mut entries: Vec<(String, T)>) -> Vec<(String, T)> {
    if entries.iter().any(|(k, _)| index_key(k).is_some()) {
        entries.sort_by_key(|(k, _)| index_key(k).map_or((1, 0), |n| (0, n)));
    }
    entries
}

impl JsValue {
    /// `v.key` for an object; anything else reads as `undefined`.
    pub fn get(&self, key: &str) -> Option<&JsValue> {
        match self {
            Self::Obj(o) => o.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[JsValue]> {
        match self {
            Self::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// A plain object (not an array, not null).
    pub fn as_obj(&self) -> Option<&JsObject> {
        match self {
            Self::Obj(o) => Some(o),
            _ => None,
        }
    }

    pub fn is_true(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// JS truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Arr(_) | Self::Obj(_) => true,
        }
    }

    /// `JSON.stringify(v)` or `JSON.stringify(v, null, 2)`.
    pub fn stringify(&self, pretty: bool) -> String {
        let mut w = Writer::new(pretty);
        self.write(&mut w);
        w.finish()
    }

    /// Write this value at the writer's current position.
    pub fn write(&self, w: &mut Writer) {
        match self {
            Self::Null => w.null(),
            Self::Bool(b) => w.bool(*b),
            Self::Num(n) => w.num(*n),
            Self::Str(s) => w.text(s),
            Self::Arr(items) => {
                w.begin_arr();
                for v in items {
                    v.write(w.elem());
                }
                w.end_arr();
            }
            Self::Obj(o) => {
                w.begin_obj();
                o.entries().for_each(|(k, v)| v.write(w.key_text(k)));
                w.end_obj();
            }
        }
    }

    /// Parse one JSON text as `JSON.parse` does, or `None` when it throws.
    pub fn parse(src: &str) -> Option<Self> {
        if let Ok(v) = parse_with(src, false) {
            return Some(v);
        }
        let fixed = json::repair(src, Overflow::Sentinel)?;
        parse_with(&fixed, true).ok()
    }
}

/// `String(v)`; `None` is `undefined`.
pub fn js_string(v: Option<&JsValue>) -> String {
    match v {
        None => "undefined".to_owned(),
        Some(JsValue::Null) => "null".to_owned(),
        Some(JsValue::Bool(b)) => b.to_string(),
        Some(JsValue::Num(n)) => num::to_js_string(*n),
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Arr(items)) => join(items, ","),
        Some(JsValue::Obj(_)) => "[object Object]".to_owned(),
    }
}

/// `items.join(sep)`: `null` items become empty strings.
pub fn join(items: &[JsValue], sep: &str) -> String {
    items
        .iter()
        .map(|v| match v {
            JsValue::Null => String::new(),
            v => js_string(Some(v)),
        })
        .collect::<Vec<_>>()
        .join(sep)
}

/// `v ?? fallback`: `None` and `null` give `None`.
pub fn defined(v: Option<&JsValue>) -> Option<&JsValue> {
    v.filter(|v| !v.is_null())
}

/// Display text of any value: strings as they are, `null`/`undefined` as
/// empty, anything else as compact JSON.
pub fn str_of(v: Option<&JsValue>) -> String {
    match v {
        None | Some(JsValue::Null) => String::new(),
        Some(JsValue::Str(s)) => s.clone(),
        Some(v) => v.stringify(false),
    }
}

fn parse_with(src: &str, repaired: bool) -> Result<JsValue, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(src);
    let v = Seed { repaired }.deserialize(&mut de)?;
    de.end()?;
    Ok(v)
}

/// Builds a [`JsValue`]. `repaired` says strings are already transcript text
/// and overflow sentinels stand for `±Infinity`.
#[derive(Clone, Copy)]
struct Seed {
    repaired: bool,
}

impl Seed {
    fn string(self, s: &str) -> String {
        if self.repaired {
            s.to_owned()
        } else {
            text::escape_reserved(s).into_owned()
        }
    }
}

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = JsValue;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<JsValue, D::Error> {
        d.deserialize_any(self)
    }
}

/// An object key, kept as transcript text.
struct KeySeed(Seed);

impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        struct V(Seed);
        impl Visitor<'_> for V {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
                Ok(self.0.string(v))
            }
        }
        d.deserialize_str(V(self.0))
    }
}

impl<'de> Visitor<'de> for Seed {
    type Value = JsValue;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_unit<E: de::Error>(self) -> Result<JsValue, E> {
        Ok(JsValue::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<JsValue, E> {
        Ok(JsValue::Bool(v))
    }
    #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<JsValue, E> {
        Ok(JsValue::Num(v as f64))
    }
    #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<JsValue, E> {
        Ok(JsValue::Num(v as f64))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<JsValue, E> {
        Ok(JsValue::Num(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<JsValue, E> {
        if self.repaired
            && let Some(n) = json::overflow_value(v)
        {
            return Ok(JsValue::Num(n));
        }
        Ok(JsValue::Str(self.string(v)))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<JsValue, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element_seed(self)? {
            items.push(v);
        }
        Ok(JsValue::Arr(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsValue, A::Error> {
        let mut entries = Vec::new();
        while let Some(k) = map.next_key_seed(KeySeed(self))? {
            let v = map.next_value_seed(self)?;
            entries.push((k, v));
        }
        Ok(JsValue::Obj(JsObject::from_entries(entries)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(src: &str) -> String {
        JsValue::parse(src)
            .map(|v| v.stringify(false))
            .unwrap_or_default()
    }

    #[test]
    fn numbers_like_js() {
        assert_eq!(
            rt("[1.0,1e2,-0,12345678901234567890,1e21,1e-7]"),
            "[1,100,0,12345678901234567000,1e+21,1e-7]"
        );
        assert_eq!(rt("[1e400,-1e400]"), "[null,null]");
    }

    #[test]
    fn key_order_like_js() {
        assert_eq!(
            rt(r#"{"b":1,"10":2,"2":3,"a":4,"b":5}"#),
            r#"{"2":3,"10":2,"b":5,"a":4}"#
        );
        assert_eq!(
            rt(r#"{"01":1,"4294967295":2,"4294967294":3}"#),
            r#"{"4294967294":3,"01":1,"4294967295":2}"#
        );
    }

    #[test]
    fn lone_surrogates_survive() {
        assert_eq!(rt(r#"["\ud83d","\ud83d\ude00"]"#), "[\"\\ud83d\",\"😀\"]");
        assert_eq!(JsValue::parse("{"), None);
    }

    #[test]
    fn pretty_and_strings() {
        let v = JsValue::parse(r#"{"a":[1,{"b":null}],"c":{}}"#).unwrap_or(JsValue::Null);
        assert_eq!(
            v.stringify(true),
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": null\n    }\n  ],\n  \"c\": {}\n}"
        );
        assert_eq!(js_string(v.get("a")), "1,[object Object]");
        let arr = JsValue::parse("[null,[1,[2,null]],true]").unwrap_or(JsValue::Null);
        assert_eq!(js_string(Some(&arr)), ",1,2,,true");
        assert_eq!(str_of(None), "");
        assert_eq!(str_of(Some(&JsValue::Num(1.5))), "1.5");
    }
}
