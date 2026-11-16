//! The TypeScript tokenizer: comments, strings, templates and regex
//! literals by hand, then keyword, number and name patterns.

use super::patterns::{self, Pattern, group, pat};
use super::{Range, TokenClass as T};
use crate::js::regex::LazyRe;
use crate::js::text::{is_js_whitespace, trim_end};

static PROPERTY: LazyRe = crate::js_re!(r"(?<!\.)\.([A-Za-z_$][\w$]*)", "g");
static DECORATOR: LazyRe = crate::js_re!(r"@[A-Za-z_$][\w$]*", "g");
static KEYWORD: LazyRe = crate::js_re!(
    r"\b(?:abstract|as|asserts|async|await|break|case|catch|class|const|continue|debugger|declare|default|delete|do|else|enum|export|extends|finally|for|from|function|get|if|implements|import|in|infer|instanceof|interface|is|keyof|let|module|namespace|new|of|override|package|private|protected|public|readonly|return|satisfies|set|static|super|switch|this|throw|try|type|typeof|using|var|while|with|yield)\b",
    "g"
);
static LITERAL: LazyRe = crate::js_re!(r"\b(?:false|null|true|undefined|NaN|Infinity)\b", "g");
static NUMBER: LazyRe = crate::js_re!(
    r"\b(?:0x[\da-f](?:_?[\da-f])*n?|0b[01](?:_?[01])*n?|0o[0-7](?:_?[0-7])*n?|\d(?:_?\d)*(?:\.\d(?:_?\d)*)?(?:e[+-]?\d(?:_?\d)*)?n?)\b",
    "gi"
);
static FUNCTION_DECL: LazyRe = crate::js_re!(r"\bfunction\s*\*?\s*([A-Za-z_$][\w$]*)", "g");
static CALL: LazyRe = crate::js_re!(r"(^|[^.A-Za-z0-9_$])([A-Za-z_$][\w$]*)(?=\s*\()", "g");
static TYPE: LazyRe = crate::js_re!(
    r"\b(?:Array|Record|Promise|Readonly|Set|Map|WeakMap|WeakSet|string|number|boolean|bigint|symbol|object|unknown|never|void|any|[A-Z][A-Za-z0-9_$]*)\b",
    "g"
);

static SEMANTIC: [Pattern; 8] = [
    group(&PROPERTY, 1, T::Property),
    pat(&DECORATOR, T::Function),
    pat(&KEYWORD, T::Keyword),
    pat(&LITERAL, T::Literal),
    pat(&NUMBER, T::Number),
    group(&FUNCTION_DECL, 1, T::Function),
    group(&CALL, 2, T::Function),
    pat(&TYPE, T::Type),
];

const fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

pub(super) fn collect(code: &str) -> Vec<Range> {
    let mut initial = Vec::new();
    initial_ranges(code, &mut initial);
    patterns::collect(code, &SEMANTIC, initial)
}

fn initial_ranges(code: &str, ranges: &mut Vec<Range>) {
    let b = code.as_bytes();
    let mut braces: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == b'/' && next == Some(b'/') {
            let end = line_end(code, i + 2);
            ranges.push(Range::new(i, end, T::Comment));
            i = end;
            continue;
        }
        if c == b'/' && next == Some(b'*') {
            let end = code[i + 2..].find("*/").map_or(b.len(), |p| i + 2 + p + 2);
            ranges.push(Range::new(i, end, T::Comment));
            i = end;
            continue;
        }
        if c == b'\'' || c == b'"' {
            let end = quoted_end(b, i, c);
            ranges.push(Range::new(i, end, T::String));
            i = end;
            continue;
        }
        if c == b'`' {
            i = template(code, i, ranges);
            continue;
        }
        if c == b'/' && is_regex_start(code, i) {
            let end = regex_end(b, i);
            if end > i + 1 {
                ranges.push(Range::new(i, end, T::Literal));
                i = end;
                continue;
            }
        }
        if c == b'{' {
            let before = previous_code_index(code, i, ranges);
            let opens = before.is_some_and(|at| {
                let mut start = at;
                while start > 0 && is_word(b[start - 1]) {
                    start -= 1;
                }
                let word = if is_word(b[at]) {
                    &b[start..=at]
                } else {
                    &[][..]
                };
                b"=(:,[".contains(&b[at])
                    || matches!(word, b"return" | b"yield" | b"const" | b"let" | b"var")
            });
            braces.push(opens);
        } else if c == b'}' {
            braces.pop();
        } else if braces.last() == Some(&true)
            && (c.is_ascii_alphabetic() || c == b'_' || c == b'$')
        {
            let before = previous_code_index(code, i, ranges);
            if before.is_some_and(|at| b"{,;".contains(&b[at])) {
                let mut end = i + 1;
                while end < b.len() && is_word(b[end]) {
                    end += 1;
                }
                if b.get(skip_trivia(code, end)) == Some(&b':') {
                    ranges.push(Range::new(i, end, T::Property));
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// `end + /^(?:\s|\/\*[\s\S]*?\*\/|\/\/[^\n]*(?:\n|$))*/.length`.
fn skip_trivia(code: &str, mut at: usize) -> usize {
    loop {
        let rest = &code[at..];
        if let Some(c) = rest.chars().next().filter(|c| is_js_whitespace(*c)) {
            at += c.len_utf8();
        } else if rest.starts_with("/*")
            && let Some(p) = rest[2..].find("*/")
        {
            at += 2 + p + 2;
        } else if rest.starts_with("//") {
            at += rest.find('\n').map_or(rest.len(), |p| p + 1);
        } else {
            return at;
        }
    }
}

fn template(code: &str, start: usize, ranges: &mut Vec<Range>) -> usize {
    let b = code.as_bytes();
    let mut seg = start;
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == b'`' {
            ranges.push(Range::new(seg, i + 1, T::String));
            return i + 1;
        }
        if b[i] == b'$' && b.get(i + 1) == Some(&b'{') {
            if seg < i {
                ranges.push(Range::new(seg, i, T::String));
            }
            ranges.push(Range::new(i, i + 2, T::Operator));
            let end = interpolation_end(code, i + 2);
            let expr_end = end.unwrap_or(b.len());
            ranges.extend(patterns::offset(collect(&code[i + 2..expr_end]), i + 2));
            let Some(end) = end else {
                return b.len();
            };
            ranges.push(Range::new(end, end + 1, T::Operator));
            i = end + 1;
            seg = i;
            continue;
        }
        i += 1;
    }
    if seg < b.len() {
        ranges.push(Range::new(seg, b.len(), T::String));
    }
    b.len()
}

fn interpolation_end(code: &str, start: usize) -> Option<usize> {
    let b = code.as_bytes();
    let mut depth = 1;
    let mut i = start;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == b'\'' || c == b'"' {
            i = quoted_end(b, i, c);
            continue;
        }
        if c == b'`' {
            i = skip_template(code, i);
            continue;
        }
        if c == b'/' && next == Some(b'/') {
            i = line_end(code, i + 2);
            continue;
        }
        if c == b'/' && next == Some(b'*') {
            i = code[i + 2..].find("*/").map_or(b.len(), |p| i + 2 + p + 2);
            continue;
        }
        if c == b'/' && is_regex_start(code, i) {
            i = regex_end(b, i);
            continue;
        }
        if c == b'{' {
            depth += 1;
        }
        if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn skip_template(code: &str, start: usize) -> usize {
    let b = code.as_bytes();
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
        } else if b[i] == b'`' {
            return i + 1;
        } else if b[i] == b'$' && b.get(i + 1) == Some(&b'{') {
            i = interpolation_end(code, i + 2).map_or(b.len(), |e| e + 1);
        } else {
            i += 1;
        }
    }
    b.len()
}

const REGEX_KEYWORDS: [&str; 10] = [
    "case",
    "delete",
    "in",
    "instanceof",
    "of",
    "return",
    "throw",
    "typeof",
    "void",
    "yield",
];

fn is_regex_start(code: &str, index: usize) -> bool {
    let before = trim_end(&code[..index]);
    let Some(last) = before.bytes().last() else {
        return true;
    };
    let keyword = REGEX_KEYWORDS.iter().any(|kw| {
        before.ends_with(kw)
            && before
                .len()
                .checked_sub(kw.len() + 1)
                .is_none_or(|p| !is_word(before.as_bytes()[p]) || before.as_bytes()[p] == b'$')
    });
    keyword || b"=([{,:;!&|?~+*%^<>-".contains(&last)
}

fn regex_end(b: &[u8], start: usize) -> usize {
    let mut class = false;
    let mut i = start + 1;
    while i < b.len() && b[i] != b'\n' {
        match b[i] {
            b'\\' => i += 2,
            b'[' => {
                class = true;
                i += 1;
            }
            b']' => {
                class = false;
                i += 1;
            }
            b'/' if !class => {
                i += 1;
                while b.get(i).is_some_and(u8::is_ascii_alphabetic) {
                    i += 1;
                }
                return i;
            }
            _ => i += 1,
        }
    }
    start + 1
}

fn quoted_end(b: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
        } else if b[i] == quote {
            return i + 1;
        } else if b[i] == b'\n' {
            return i;
        } else {
            i += 1;
        }
    }
    b.len()
}

fn line_end(code: &str, start: usize) -> usize {
    code[start..].find('\n').map_or(code.len(), |p| start + p)
}

/// The char before `end`: its start byte and value.
pub(super) fn prev_char(code: &str, end: usize) -> Option<(usize, char)> {
    code[..end].char_indices().next_back()
}

/// Start byte of the last char before `index` that is neither whitespace
/// nor inside a comment.
fn previous_code_index(code: &str, index: usize, ranges: &[Range]) -> Option<usize> {
    let mut cur = prev_char(code, index);
    let mut ri = ranges.len();
    while let Some((pos, ch)) = cur {
        if is_js_whitespace(ch) {
            cur = prev_char(code, pos);
            continue;
        }
        if ri > 0 && ranges[ri - 1].end > pos {
            ri -= 1;
            let r = ranges[ri];
            if r.class == T::Comment && r.start <= pos {
                cur = prev_char(code, r.start);
            }
            continue;
        }
        break;
    }
    cur.map(|(p, _)| p)
}
