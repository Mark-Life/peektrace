//! One Claude JSONL line, decoded into the few fields a header needs.
//!
//! Each field keeps JavaScript's loose reading of `JSON.parse` output: a
//! field of the wrong type reads as absent, a duplicate key keeps the last
//! value, and everything else is skipped without being built.

use std::borrow::Cow;
use std::cell::Cell;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::js::json;

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
                Ok(Str(overflow(v).is_none().then_some(Cow::Borrowed(v))))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Str(overflow(v).is_none().then(|| Cow::Owned(v.to_owned()))))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(Str(overflow(&v).is_none().then_some(Cow::Owned(v))))
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
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Num(overflow(v)))
            }
            otherwise!(Num(None); bool, unit, seq, map);
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

thread_local! {
    /// Set while a repaired line is parsed, so overflow sentinels read as
    /// numbers there and as plain strings everywhere else.
    static REPAIRED: Cell<bool> = const { Cell::new(false) };
}

/// `±Infinity` when `s` stands for an out-of-range number in a repaired line.
fn overflow(s: &str) -> Option<f64> {
    json::overflow_value(s).filter(|_| REPAIRED.get())
}

/// Parse one line as strict JSON. `Err` marks the line malformed.
pub fn parse_line(text: &str) -> Result<Line<'_>, serde_json::Error> {
    serde_json::from_str(text)
}

/// Parse a line rewritten by [`json::repair`] with [`json::Overflow::Sentinel`].
pub fn parse_repaired(text: &str) -> Result<Line<'_>, serde_json::Error> {
    REPAIRED.set(true);
    let line = serde_json::from_str(text);
    REPAIRED.set(false);
    line
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
    fn out_of_range_numbers_read_as_infinity() {
        let raw = r#"{"type":"assistant","timestamp":1e400,"message":{"model":"m","usage":{"input_tokens":1e400,"output_tokens":-1e400}}}"#;
        assert!(parse_line(raw).is_err());
        let fixed = json::repair(raw, json::Overflow::Sentinel).expect("repaired");
        let line = parse_repaired(&fixed).expect("valid");
        assert!(line.message.model.is("m"));
        assert_eq!(line.timestamp.get(), None);
        assert_eq!(line.message.usage.input_tokens.0, Some(f64::INFINITY));
        assert_eq!(line.message.usage.output_tokens.0, Some(f64::NEG_INFINITY));
        let strict = "{\"timestamp\":\"\u{10F7FF}+\"}";
        let line = parse_line(strict).expect("valid");
        assert_eq!(line.timestamp.get(), Some("\u{10F7FF}+"));
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
