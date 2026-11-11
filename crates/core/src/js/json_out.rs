//! `JSON.stringify` output, compact or with a 2-space indent, written field
//! by field so callers control key order.

use std::fmt::Write as _;

use super::num;
use super::text::{self, Unit};

/// Append `units` as a JSON string, escaped the way `JSON.stringify` does.
fn push_units(out: &mut String, units: impl Iterator<Item = Unit>) {
    out.push('"');
    for u in units {
        match u {
            Unit::Char('"') => out.push_str("\\\""),
            Unit::Char('\\') => out.push_str("\\\\"),
            Unit::Char('\u{08}') => out.push_str("\\b"),
            Unit::Char('\u{0C}') => out.push_str("\\f"),
            Unit::Char('\n') => out.push_str("\\n"),
            Unit::Char('\r') => out.push_str("\\r"),
            Unit::Char('\t') => out.push_str("\\t"),
            Unit::Char(c) if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            Unit::Char(c) => out.push(c),
            Unit::Lone(unit) => {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out.push('"');
}

/// Append a plain string as JSON.
pub fn push_str(out: &mut String, s: &str) {
    push_units(out, s.chars().map(Unit::Char));
}

/// Append transcript text as JSON; unpaired surrogates print as `\udXXX`.
pub fn push_text(out: &mut String, s: &str) {
    push_units(out, text::units(s));
}

/// Builds one JSON document. Containers track whether they hold an item yet,
/// so separators and indentation come out as `JSON.stringify` writes them.
#[derive(Debug)]
pub struct Writer {
    out: String,
    pretty: bool,
    /// One flag per open container: true once it holds an item.
    open: Vec<bool>,
}

impl Writer {
    /// `JSON.stringify(v)` (`pretty = false`) or `JSON.stringify(v, null, 2)`.
    pub fn new(pretty: bool) -> Self {
        Self {
            out: String::new(),
            pretty,
            open: Vec::new(),
        }
    }

    pub fn finish(self) -> String {
        self.out
    }

    fn newline(&mut self, depth: usize) {
        if self.pretty {
            self.out.push('\n');
            (0..depth).for_each(|_| self.out.push_str("  "));
        }
    }

    /// Separator and indent before the next item of the open container.
    fn item(&mut self) {
        let depth = self.open.len();
        if let Some(has) = self.open.last_mut() {
            if *has {
                self.out.push(',');
            }
            *has = true;
            self.newline(depth);
        }
    }

    fn close(&mut self, bracket: char) {
        if self.open.pop() == Some(true) {
            self.newline(self.open.len());
        }
        self.out.push(bracket);
    }

    pub fn begin_obj(&mut self) {
        self.out.push('{');
        self.open.push(false);
    }

    pub fn end_obj(&mut self) {
        self.close('}');
    }

    pub fn begin_arr(&mut self) {
        self.out.push('[');
        self.open.push(false);
    }

    pub fn end_arr(&mut self) {
        self.close(']');
    }

    /// Start an array element; write its value next.
    pub fn elem(&mut self) -> &mut Self {
        self.item();
        self
    }

    /// Start an object member with a plain key; write its value next.
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.item();
        push_str(&mut self.out, k);
        self.out.push_str(if self.pretty { ": " } else { ":" });
        self
    }

    /// Start an object member whose key is transcript text.
    pub fn key_text(&mut self, k: &str) -> &mut Self {
        self.item();
        push_text(&mut self.out, k);
        self.out.push_str(if self.pretty { ": " } else { ":" });
        self
    }

    /// Plain string value.
    pub fn str(&mut self, s: &str) {
        push_str(&mut self.out, s);
    }

    /// Transcript text value.
    pub fn text(&mut self, s: &str) {
        push_text(&mut self.out, s);
    }

    pub fn num(&mut self, x: f64) {
        self.out.push_str(&num::to_json(x));
    }

    pub fn int(&mut self, n: i64) {
        let _ = write!(self.out, "{n}");
    }

    pub fn bool(&mut self, b: bool) {
        self.out.push_str(if b { "true" } else { "false" });
    }

    pub fn null(&mut self) {
        self.out.push_str("null");
    }

    /// An array of transcript text.
    pub fn texts<'a>(&mut self, items: impl IntoIterator<Item = &'a str>) {
        self.begin_arr();
        items.into_iter().for_each(|s| self.elem().text(s));
        self.end_arr();
    }

    /// An array of numbers.
    pub fn nums(&mut self, items: impl IntoIterator<Item = f64>) {
        self.begin_arr();
        items.into_iter().for_each(|x| self.elem().num(x));
        self.end_arr();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(pretty: bool) -> String {
        let mut w = Writer::new(pretty);
        w.begin_obj();
        w.key("a").int(1);
        w.key("b").begin_arr();
        w.elem().text("x\u{10F800}");
        w.end_arr();
        w.key("c").begin_arr();
        w.end_arr();
        w.key("d").begin_obj();
        w.end_obj();
        w.end_obj();
        w.finish()
    }

    #[test]
    fn matches_json_stringify() {
        assert_eq!(doc(false), r#"{"a":1,"b":["x\ud800"],"c":[],"d":{}}"#);
        assert_eq!(
            doc(true),
            "{\n  \"a\": 1,\n  \"b\": [\n    \"x\\ud800\"\n  ],\n  \"c\": [],\n  \"d\": {}\n}"
        );
    }

    #[test]
    fn escapes() {
        let mut out = String::new();
        push_str(&mut out, "a\"b\\c\n\u{1}\u{7f}\u{2028}é");
        assert_eq!(out, "\"a\\\"b\\\\c\\n\\u0001\u{7f}\u{2028}é\"");
    }
}
