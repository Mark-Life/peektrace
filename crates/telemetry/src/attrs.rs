//! Span attributes as the JSON each value prints as, shared by the `--otel`
//! echo and the wide-event store so both encode values the same way.

use std::fmt;

use peektrace_core::js::json_out::push_str;
use peektrace_core::js::num;
use tracing::field::{Field, Visit};

/// Field that marks a span failed; its value is the failure message. It is
/// never an attribute: TS takes the outcome from the span's exit.
pub const FAIL_FIELD: &str = "error";
/// Marks a span that starts its own wide event.
pub const ROOT_FIELD: &str = "peektrace.root";
/// The event kind of a root span.
pub const KIND_FIELD: &str = "peektrace.kind";

/// Attributes in first-set order (a re-set key keeps its place), the
/// failure message, and the root markers.
#[derive(Debug, Default, Clone)]
pub struct Attrs {
    fields: Vec<(&'static str, String)>,
    error: Option<String>,
    root: bool,
    kind: Option<String>,
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    push_str(&mut out, s);
    out
}

impl Attrs {
    fn set(&mut self, key: &'static str, json: String) {
        match self.fields.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = json,
            None => self.fields.push((key, json)),
        }
    }

    fn set_text(&mut self, key: &'static str, text: String) {
        if key == FAIL_FIELD {
            self.error = Some(text);
            return;
        }
        if key == KIND_FIELD {
            self.kind = Some(text.clone());
        }
        self.set(key, quoted(&text));
    }

    /// `(key, json)` pairs in order.
    pub fn fields(&self) -> &[(&'static str, String)] {
        &self.fields
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_root(&self) -> bool {
        self.root
    }

    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    /// The attributes as one JSON object.
    pub fn to_json(&self) -> String {
        object(self.fields.iter().map(|(k, v)| (*k, v.as_str())))
    }
}

/// A JSON object from `(key, json)` pairs.
pub fn object<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in pairs.enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_str(&mut out, k);
        out.push(':');
        out.push_str(v);
    }
    out.push('}');
    out
}

impl Visit for Attrs {
    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == ROOT_FIELD {
            self.root = value;
        }
        self.set(field.name(), value.to_string());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.set(field.name(), value.to_string());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.set(field.name(), value.to_string());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.set(field.name(), num::to_json(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set_text(field.name(), value.to_owned());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.set_text(field.name(), format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_escape_keys() {
        assert_eq!(
            object([("a\"b", "1"), ("c", "\"x\"")].into_iter()),
            r#"{"a\"b":1,"c":"x"}"#
        );
        assert_eq!(object(std::iter::empty()), "{}");
    }
}
