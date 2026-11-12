//! JSON and Markdown: pattern languages; Markdown also tokenizes fenced
//! code in the fence's language.

use super::patterns::{self, Class, Pattern, pat};
use super::{Def, Range, TokenClass as T, def_named, ranges_of};
use crate::js::regex::{Caps, LazyRe, Re};
use crate::js::text;

static JSON_STRING: LazyRe = crate::js_re!(
    r#"\/\/[^\n]*|\/\*[\s\S]*?\*\/|("(?:\\.|[^"\\])*")(?=\s*:)|"(?:\\.|[^"\\])*""#,
    "g"
);
static JSON_LITERAL: LazyRe = crate::js_re!(r"\b(?:false|null|true)\b", "g");
static JSON_NUMBER: LazyRe = crate::js_re!(r"-?\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b", "gi");

fn json_class(c: &Caps<'_>) -> T {
    let all = c.all();
    if all.starts_with("//") || all.starts_with("/*") {
        T::Comment
    } else if c.range(1).is_some() {
        T::Property
    } else {
        T::String
    }
}

static JSON: [Pattern; 3] = [
    Pattern {
        re: &JSON_STRING,
        group: 0,
        class: Class::By(json_class),
    },
    pat(&JSON_LITERAL, T::Literal),
    pat(&JSON_NUMBER, T::Number),
];

pub(super) fn json(code: &str) -> Vec<Range> {
    patterns::collect(code, &JSON, Vec::new())
}

static HEADING: LazyRe = crate::js_re!(r"^#{1,6}\s.*$", "gm");
static SETEXT: LazyRe = crate::js_re!(r"^.+\n(?:=+|-+)\s*$", "gm");
static CODE_INLINE: LazyRe = crate::js_re!(r"`[^`\n]+`", "g");
static LINK: LazyRe = crate::js_re!(r"!?\[[^\]]+\]\([^)]+\)", "g");
static LIST_META: LazyRe = crate::js_re!(r"^\s{0,3}(?:>|[-*+] |\d+\. )", "gm");
static FENCE_OPEN: LazyRe = crate::js_re!(r"^( {0,3})(`{3,}|~{3,})\s*([^\n]*)$", "gm");

static MARKDOWN: [Pattern; 5] = [
    pat(&HEADING, T::Heading),
    pat(&SETEXT, T::Heading),
    pat(&CODE_INLINE, T::CodeInline),
    pat(&LINK, T::Link),
    pat(&LIST_META, T::Meta),
];

pub(super) fn markdown(code: &str, depth: usize) -> Vec<Range> {
    patterns::collect(code, &MARKDOWN, fenced(code, depth))
}

/// `info.trim().split(/\s+/)[0]`.
fn first_word(info: &str) -> &str {
    text::trim(info)
        .split(text::is_js_whitespace)
        .next()
        .unwrap_or("")
}

fn fenced(code: &str, depth: usize) -> Vec<Range> {
    let mut ranges = Vec::new();
    let mut last = 0;
    while last <= code.len() {
        let Some(open) = FENCE_OPEN.captures_at(code, last) else {
            break;
        };
        let marker = open.get(2).unwrap_or("```");
        let Some(nl) = code[open.end()..].find('\n') else {
            break;
        };
        let content_start = open.end() + nl;
        let fence = marker.as_bytes()[0] as char;
        let closing = Re::new(&format!(r"^ {{0,3}}{fence}{{{},}}\s*$", marker.len()), "gm");
        let close = closing.captures_at(code, content_start + 1);
        let content_end = close.as_ref().map_or(code.len(), Caps::start);
        let info = first_word(open.get(3).unwrap_or(""));
        let embedded = if info.is_empty() {
            Vec::new()
        } else {
            def_named(info).map_or_else(Vec::new, |def: Def| {
                let body = &code[content_start + 1..content_end];
                patterns::offset(ranges_of(body, def, depth + 1), content_start + 1).collect()
            })
        };
        ranges.push(Range::new(open.start(), content_start, T::Meta));
        fill_code(&mut ranges, content_start + 1, content_end, embedded);
        if let Some(c) = &close {
            ranges.push(Range::new(c.start(), c.end(), T::Meta));
        }
        last = close.map_or(code.len() + 1, |c| c.end());
    }
    ranges
}

fn fill_code(ranges: &mut Vec<Range>, start: usize, end: usize, embedded: Vec<Range>) {
    let mut at = start;
    for r in embedded {
        if r.start > at {
            ranges.push(Range::new(at, r.start, T::CodeInline));
        }
        at = r.end;
        ranges.push(r);
    }
    if at < end {
        ranges.push(Range::new(at, end, T::CodeInline));
    }
}
