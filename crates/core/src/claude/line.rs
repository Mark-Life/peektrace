//! One Claude JSONL line, decoded into the few fields a header needs.
//!
//! Each field keeps JavaScript's loose reading of `JSON.parse` output: a
//! field of the wrong type reads as absent, a duplicate key keeps the last
//! value, and everything else is skipped without being built.

use std::borrow::Cow;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::js::text;

/// Answer `$value` for every listed JSON shape, consuming nested values.
macro_rules! otherwise {
    ($value:expr; $($kind:ident),*) => {
        $(otherwise!(@one $kind, $value);)*
    };
    (@one bool, $value:expr) => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> { Ok($value) }
    };
    (@one num, $value:expr) => {
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> { Ok($value) }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> { Ok($value) }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> { Ok($value) }
    };
    (@one str, $value:expr) => {
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> { Ok($value) }
    };
    (@one unit, $value:expr) => {
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> { Ok($value) }
    };
    (@one seq, $value:expr) => {
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok($value)
        }
    };
    (@one map, $value:expr) => {
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok($value)
        }
    };
}

/// `Deserialize` for a struct with an `is_object` flag: an object fills the
/// listed fields, an array sets `is_object` to `$arrays` (JS `typeof [] ===
/// "object"`), and any other value gives the default.
macro_rules! loose_object {
    ($ty:ty, arrays: $arrays:literal, { $($key:literal => $field:ident),* $(,)? }) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = $ty;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("any JSON value")
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                        let mut out = <$ty>::default();
                        out.is_object = true;
                        while let Some(key) = map.next_key::<Str<'_>>()? {
                            match key.0.as_deref() {
                                $(Some($key) => out.$field = map.next_value()?,)*
                                _ => {
                                    map.next_value::<IgnoredAny>()?;
                                }
                            }
                        }
                        Ok(out)
                    }
                    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                        while seq.next_element::<IgnoredAny>()?.is_some() {}
                        let mut out = <$ty>::default();
                        out.is_object = $arrays;
                        Ok(out)
                    }
                    otherwise!(<$ty>::default(); bool, num, str, unit);
                }
                d.deserialize_any(V)
            }
        }
    };
}

/// A value that counts only when it is a JSON string.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Str<'a>(pub Option<Cow<'a, str>>);

impl Str<'_> {
    /// The string, when the value was one.
    pub fn get(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// The string when it is non-empty (JS truthiness).
    pub fn non_empty(&self) -> Option<&str> {
        self.get().filter(|s| !s.is_empty())
    }

    pub fn is(&self, want: &str) -> bool {
        self.get() == Some(want)
    }
}

impl<'de> Deserialize<'de> for Str<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Str<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(Str(Some(Cow::Borrowed(v))))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Str(Some(Cow::Owned(v.to_owned()))))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(Str(Some(Cow::Owned(v))))
            }
            otherwise!(Str(None); bool, num, unit, seq, map);
        }
        d.deserialize_any(V)
    }
}

/// A value that counts only when it is a JSON number.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Num(pub Option<f64>);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Num;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }
            #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Num(Some(v as f64)))
            }
            #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Num(Some(v as f64)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Num(Some(v)))
            }
            otherwise!(Num(None); bool, str, unit, seq, map);
        }
        d.deserialize_any(V)
    }
}

/// True only for JSON `true` (`=== true`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IsTrue(pub bool);

impl<'de> Deserialize<'de> for IsTrue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = IsTrue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(IsTrue(v))
            }
            otherwise!(IsTrue(false); num, str, unit, seq, map);
        }
        d.deserialize_any(V)
    }
}

/// `usage.cache_creation`: only the 1-hour write count matters.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CacheSplit {
    pub is_object: bool,
    pub ephemeral_1h_input_tokens: Num,
}

loose_object!(CacheSplit, arrays: false, {
    "ephemeral_1h_input_tokens" => ephemeral_1h_input_tokens,
});

/// `message.usage`. `is_object` is JS `usage && typeof usage === "object"`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Usage {
    pub is_object: bool,
    pub input_tokens: Num,
    pub cache_read_input_tokens: Num,
    pub cache_creation_input_tokens: Num,
    pub output_tokens: Num,
    pub cache_creation: CacheSplit,
}

loose_object!(Usage, arrays: true, {
    "input_tokens" => input_tokens,
    "cache_read_input_tokens" => cache_read_input_tokens,
    "cache_creation_input_tokens" => cache_creation_input_tokens,
    "output_tokens" => output_tokens,
    "cache_creation" => cache_creation,
});

/// One content block: only its `type` matters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Block<'a> {
    is_object: bool,
    kind: Str<'a>,
}

loose_object!(Block<'de>, arrays: false, { "type" => kind });

/// True when `message.content` is a non-empty array whose every item is an
/// object with `type: "tool_result"`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ToolResultsOnly(pub bool);

impl<'de> Deserialize<'de> for ToolResultsOnly {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ToolResultsOnly;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut any = false;
                let mut all = true;
                while let Some(block) = seq.next_element::<Block<'de>>()? {
                    any = true;
                    all &= block.is_object && block.kind.is("tool_result");
                }
                Ok(ToolResultsOnly(any && all))
            }
            otherwise!(ToolResultsOnly(false); bool, num, str, unit, map);
        }
        d.deserialize_any(V)
    }
}

/// `line.message`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Message<'a> {
    pub is_object: bool,
    pub model: Str<'a>,
    pub usage: Usage,
    pub content: ToolResultsOnly,
}

loose_object!(Message<'de>, arrays: false, {
    "model" => model,
    "usage" => usage,
    "content" => content,
});

/// The header fields of one JSONL row. `is_object` is false for rows that
/// parse to anything but an object; those carry no fields.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Line<'a> {
    pub is_object: bool,
    pub kind: Str<'a>,
    pub timestamp: Str<'a>,
    pub cwd: Str<'a>,
    pub git_branch: Str<'a>,
    pub ai_title: Str<'a>,
    pub request_id: Str<'a>,
    pub is_meta: IsTrue,
    pub is_compact_summary: IsTrue,
    pub message: Message<'a>,
}

loose_object!(Line<'de>, arrays: false, {
    "type" => kind,
    "timestamp" => timestamp,
    "cwd" => cwd,
    "gitBranch" => git_branch,
    "aiTitle" => ai_title,
    "requestId" => request_id,
    "isMeta" => is_meta,
    "isCompactSummary" => is_compact_summary,
    "message" => message,
});

/// Parse one line as strict JSON. `Err` marks the line malformed.
pub fn parse_line(text: &str) -> Result<Line<'_>, serde_json::Error> {
    serde_json::from_str(text)
}

/// The UTF-16 unit of the `\uXXXX` escape at `at`, if there is one.
fn escape_at(b: &[u8], at: usize) -> Option<u16> {
    let hex = b.get(at..at + 6)?.strip_prefix(b"\\u")?;
    let hex = std::str::from_utf8(hex).ok()?;
    u16::from_str_radix(hex, 16).ok()
}

/// The code point a `\\uHIGH\\uLOW` escape pair at `at` encodes.
fn pair_at(b: &[u8], at: usize) -> Option<char> {
    let high = escape_at(b, at).filter(|u| (0xD800..0xDC00).contains(u))?;
    let low = escape_at(b, at + 6).filter(|u| (0xDC00..0xE000).contains(u))?;
    char::decode_utf16([high, low]).next()?.ok()
}

/// `text` with each unpaired surrogate escape (`\\ud800`) replaced by its
/// stand-in char, or `None` when there is none. `JSON.parse` accepts unpaired
/// surrogates; `serde_json` rejects them in strings it decodes. Strings parsed
/// from the result are already transcript text: real chars that could pass
/// for a stand-in are escaped too.
pub fn replace_lone_surrogates(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut lone = false;
    let mut copied = 0;
    let mut at = 0;
    while at < b.len() {
        // A reserved char, raw (UTF-8 lead byte 0xF4) or as an escape pair.
        let literal = match b[at] {
            0xF4 => text[at..].chars().next().map(|c| (c, c.len_utf8())),
            b'\\' => pair_at(b, at).map(|c| (c, 12)),
            _ => None,
        };
        if let Some((c, len)) = literal {
            if text::reserved(c) {
                out.push_str(&text[copied..at]);
                text::push_literal(&mut out, c);
                copied = at + len;
            }
            at += len;
            continue;
        }
        if b[at] != b'\\' {
            at += 1;
            continue;
        }
        match escape_at(b, at).map(|unit| (unit, text::surrogate_stand_in(unit))) {
            None => at += 2,
            Some((_, Some(stand_in))) => {
                out.push_str(&text[copied..at]);
                out.push(stand_in);
                lone = true;
                at += 6;
                copied = at;
            }
            Some((_, None)) => at += 6,
        }
    }
    out.push_str(&text[copied..]);
    lone.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fields_loosely() {
        let line = parse_line(
            r#"{"type":"user","timestamp":5,"cwd":"/a","cwd":"/b","isMeta":"true","message":{"content":[{"type":"tool_result"}],"usage":[]}}"#,
        )
        .expect("valid");
        assert!(line.is_object);
        assert!(line.kind.is("user"));
        assert_eq!(line.timestamp.get(), None);
        assert_eq!(line.cwd.get(), Some("/b"));
        assert!(!line.is_meta.0);
        assert!(line.message.is_object);
        assert!(line.message.content.0);
        assert!(line.message.usage.is_object);
    }

    #[test]
    fn content_checks() {
        let only = |c: &str| {
            parse_line(&format!(r#"{{"message":{{"content":{c}}}}}"#))
                .expect("valid")
                .message
                .content
                .0
        };
        assert!(!only("[]"));
        assert!(!only(r#""text""#));
        assert!(!only(r#"[{"type":"tool_result"},{"type":"text"}]"#));
        assert!(!only(r#"[{"type":"tool_result"},null]"#));
        assert!(only(r#"[{"type":"text","type":"tool_result"}]"#));
    }

    #[test]
    fn non_objects_and_errors() {
        assert!(!parse_line("42").expect("valid").is_object);
        assert!(!parse_line("[1,{}]").expect("valid").is_object);
        assert!(!parse_line("null\r").expect("valid").is_object);
        assert!(parse_line("{\"a\":\"tab\there\"}").is_err());
        assert!(parse_line("{\"a\":1} x").is_err());
        assert!(parse_line("{\"a\":1,}").is_err());
        assert!(
            parse_line(r#"{"type":"user"}"#)
                .expect("valid")
                .kind
                .is("user")
        );
    }

    #[test]
    fn reserved_chars_escape_beside_lone_surrogates() {
        let raw = "{\"a\":\"\\ud801\u{10F801}\\udbfe\\udc01\\ud83d\\ude00\"}";
        assert_eq!(
            replace_lone_surrogates(raw).as_deref(),
            Some("{\"a\":\"\u{10F801}\u{10F7FF}\u{10F801}\u{10F7FF}\u{10F801}\\ud83d\\ude00\"}")
        );
    }

    #[test]
    fn lone_surrogates() {
        assert_eq!(
            replace_lone_surrogates(r#"{"a":"\ud800","b":"\\ud800","c":"\ud83d\ude00\udc00"}"#)
                .as_deref(),
            Some("{\"a\":\"\u{10F800}\",\"b\":\"\\\\ud800\",\"c\":\"\\ud83d\\ude00\u{10FC00}\"}")
        );
        assert_eq!(replace_lone_surrogates(r#"{"a":"\u0041"}"#), None);
    }

    #[test]
    fn usage_numbers() {
        let line = parse_line(
            r#"{"message":{"usage":{"input_tokens":3,"output_tokens":1.5,"cache_creation":{"ephemeral_1h_input_tokens":7},"cache_read_input_tokens":"9"}}}"#,
        )
        .expect("valid");
        let u = line.message.usage;
        assert_eq!(u.input_tokens.0, Some(3.0));
        assert_eq!(u.output_tokens.0, Some(1.5));
        assert_eq!(u.cache_read_input_tokens.0, None);
        assert_eq!(u.cache_creation.ephemeral_1h_input_tokens.0, Some(7.0));
    }
}
