//! JS regular expressions (no `u` flag) on Rust engines. A pattern is written
//! as in the TS source and translated: `\d`, `\w`, `\b` are ASCII, `\s` is the
//! JS whitespace set, `.` stops at every JS line terminator, and `m`-mode
//! anchors also break at `\r`. Patterns with lookaround or backreferences run
//! on `fancy-regex`, the rest on `regex`.

use std::ops::Range;

/// JS `\s`, for use inside a class.
const WS: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";
const WORD: &str = "A-Za-z0-9_";
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
/// ASCII `\b` / `\B` for `fancy-regex`, which cannot switch Unicode off inline.
const FANCY_B: &str = r"(?:(?<=[A-Za-z0-9_])(?![A-Za-z0-9_])|(?<![A-Za-z0-9_])(?=[A-Za-z0-9_]))";
const FANCY_NB: &str = r"(?:(?<=[A-Za-z0-9_])(?=[A-Za-z0-9_])|(?<![A-Za-z0-9_])(?![A-Za-z0-9_]))";

fn needs_fancy(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                if b.get(i + 1).is_some_and(|c| (b'1'..=b'9').contains(c)) {
                    return true;
                }
                i += 2;
                continue;
            }
            b'(' if b.get(i + 1) == Some(&b'?') => {
                let rest = &b[i + 2..];
                if rest.starts_with(b"=")
                    || rest.starts_with(b"!")
                    || rest.starts_with(b"<=")
                    || rest.starts_with(b"<!")
                {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// A `{` that starts a JS quantifier: `{n}`, `{n,}` or `{n,m}`.
fn is_quantifier(rest: &str) -> bool {
    let Some(end) = rest.find('}') else {
        return false;
    };
    let body = &rest[1..end];
    let (lo, hi) = body.split_once(',').unwrap_or((body, ""));
    !lo.is_empty()
        && lo.bytes().all(|c| c.is_ascii_digit())
        && hi.bytes().all(|c| c.is_ascii_digit())
}

fn push_literal(out: &mut String, c: char) {
    if c.is_ascii_punctuation() {
        out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4])));
    } else {
        out.push(c);
    }
}

/// Translate one JS escape (the char after `\`). `class` is true inside `[...]`.
fn push_escape(out: &mut String, c: char, class: bool, fancy: bool) {
    match (c, class) {
        ('d', false) => out.push_str("[0-9]"),
        ('d', true) => out.push_str("0-9"),
        ('D', _) => out.push_str("[^0-9]"),
        ('w', false) => out.push_str("[A-Za-z0-9_]"),
        ('w', true) => out.push_str(WORD),
        ('W', _) => out.push_str("[^A-Za-z0-9_]"),
        ('s', false) => {
            out.push('[');
            out.push_str(WS);
            out.push(']');
        }
        ('s', true) => out.push_str(WS),
        ('S', _) => {
            out.push_str("[^");
            out.push_str(WS);
            out.push(']');
        }
        ('b', false) => out.push_str(if fancy { FANCY_B } else { r"(?-u:\b)" }),
        ('B', false) => out.push_str(if fancy { FANCY_NB } else { r"(?-u:\B)" }),
        ('n' | 't' | 'r' | 'f' | 'v' | 'x' | 'u', _) | ('1'..='9', false) => {
            out.push('\\');
            out.push(c);
        }
        _ => push_literal(out, c),
    }
}

/// The Rust pattern for a JS pattern and its flags.
fn translate(src: &str, flags: &str, fancy: bool) -> String {
    let mut out = String::with_capacity(src.len() * 2);
    let inline: String = flags
        .chars()
        .filter_map(|f| match f {
            'i' => Some("i"),
            'm' => Some("mR"),
            'g' | 'y' | 's' => None,
            other => unreachable!("unsupported JS flag {other}"),
        })
        .collect();
    if !inline.is_empty() {
        out.push_str("(?");
        out.push_str(&inline);
        out.push(')');
    }
    let dot_all = flags.contains('s');
    let mut class = false;
    let mut chars = src.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => {
                let Some((_, e)) = chars.next() else { break };
                if e == 'u' || e == 'x' {
                    let n = if e == 'u' { 4 } else { 2 };
                    let hex: String = (0..n)
                        .filter_map(|_| chars.next().map(|(_, h)| h))
                        .collect();
                    out.push_str("\\x{");
                    out.push_str(&hex);
                    out.push('}');
                } else {
                    push_escape(&mut out, e, class, fancy);
                }
            }
            '[' if !class => {
                class = true;
                out.push('[');
                if chars.peek().is_some_and(|(_, n)| *n == '^') {
                    chars.next();
                    out.push('^');
                }
            }
            ']' if class => {
                class = false;
                out.push(']');
            }
            '[' | '&' | '~' if class => {
                out.push('\\');
                out.push(c);
            }
            '.' if !class && !dot_all => out.push_str(DOT),
            '{' if !class && is_quantifier(&src[at..]) => {
                out.push('{');
                for (_, q) in chars.by_ref() {
                    out.push(q);
                    if q == '}' {
                        break;
                    }
                }
            }
            '{' | '}' if !class => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[derive(Debug)]
enum Engine {
    Std(regex::Regex),
    Fancy(fancy_regex::Regex),
}

/// A compiled JS regular expression.
#[derive(Debug)]
pub struct Re(Engine);

/// Group spans of one match, as byte ranges into the haystack.
#[derive(Debug, Clone)]
pub struct Caps<'h> {
    hay: &'h str,
    groups: Vec<Option<Range<usize>>>,
}

impl<'h> Caps<'h> {
    pub fn get(&self, i: usize) -> Option<&'h str> {
        self.groups
            .get(i)
            .and_then(Option::as_ref)
            .map(|r| &self.hay[r.clone()])
    }

    pub fn range(&self, i: usize) -> Option<Range<usize>> {
        self.groups.get(i).cloned().flatten()
    }

    /// The whole match.
    pub fn all(&self) -> &'h str {
        self.get(0).unwrap_or("")
    }

    pub fn start(&self) -> usize {
        self.range(0).map_or(0, |r| r.start)
    }

    pub fn end(&self) -> usize {
        self.range(0).map_or(0, |r| r.end)
    }
}

impl Re {
    /// Compile a pattern from the TS source. Patterns are constants, so a
    /// bad one is a bug.
    pub fn new(src: &str, flags: &str) -> Self {
        let fancy = needs_fancy(src);
        let pattern = translate(src, flags, fancy);
        if fancy {
            Self(Engine::Fancy(
                fancy_regex::RegexBuilder::new(&pattern)
                    .backtrack_limit(10_000_000)
                    .build()
                    .unwrap_or_else(|e| unreachable!("bad pattern {src}: {e}")),
            ))
        } else {
            Self(Engine::Std(
                regex::Regex::new(&pattern)
                    .unwrap_or_else(|e| unreachable!("bad pattern {src}: {e}")),
            ))
        }
    }

    pub fn is_match(&self, s: &str) -> bool {
        match &self.0 {
            Engine::Std(r) => r.is_match(s),
            Engine::Fancy(r) => r.is_match(s).unwrap_or(false),
        }
    }

    /// First match at or after byte `start`, with lookbehind seeing the text before it.
    pub fn captures_at<'h>(&self, s: &'h str, start: usize) -> Option<Caps<'h>> {
        match &self.0 {
            Engine::Std(r) => r.captures_at(s, start).map(|c| Caps {
                hay: s,
                groups: c.iter().map(|m| m.map(|m| m.range())).collect(),
            }),
            Engine::Fancy(r) => r.captures_from_pos(s, start).ok().flatten().map(|c| Caps {
                hay: s,
                groups: c.iter().map(|m| m.map(|m| m.range())).collect(),
            }),
        }
    }

    pub fn captures<'h>(&self, s: &'h str) -> Option<Caps<'h>> {
        self.captures_at(s, 0)
    }

    pub fn find(&self, s: &str) -> Option<Range<usize>> {
        match &self.0 {
            Engine::Std(r) => r.find(s).map(|m| m.range()),
            Engine::Fancy(r) => r.find(s).ok().flatten().map(|m| m.range()),
        }
    }

    /// Every match, as a JS `g` exec loop finds them: an empty match moves on
    /// by one char.
    pub fn captures_all<'h>(&self, s: &'h str) -> Vec<Caps<'h>> {
        let mut out = Vec::new();
        let mut at = 0;
        while at <= s.len() {
            let Some(c) = self.captures_at(s, at) else {
                break;
            };
            let end = c.end();
            at = if end == c.start() {
                end + s[end..].chars().next().map_or(1, char::len_utf8)
            } else {
                end
            };
            out.push(c);
        }
        out
    }

    /// `s.replace(re, f)` for a `g` pattern.
    pub fn replace_all(&self, s: &str, mut f: impl FnMut(&Caps<'_>) -> String) -> String {
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for c in self.captures_all(s) {
            out.push_str(&s[last..c.start()]);
            out.push_str(&f(&c));
            last = c.end();
        }
        out.push_str(&s[last..]);
        out
    }

    /// `s.replace(re, to)` for a `g` pattern, `to` taken literally.
    pub fn replace_all_str(&self, s: &str, to: &str) -> String {
        self.replace_all(s, |_| to.to_owned())
    }

    /// `s.replace(re, f)` without `g`: the first match only.
    pub fn replace_first(&self, s: &str, f: impl FnOnce(&Caps<'_>) -> String) -> String {
        match self.captures(s) {
            Some(c) => {
                let mut out = String::with_capacity(s.len());
                out.push_str(&s[..c.start()]);
                out.push_str(&f(&c));
                out.push_str(&s[c.end()..]);
                out
            }
            None => s.to_owned(),
        }
    }

    /// `s.split(re)` for a pattern without capture groups.
    pub fn split<'h>(&self, s: &'h str) -> Vec<&'h str> {
        if s.is_empty() {
            return if self.find(s).is_some() {
                vec![]
            } else {
                vec![s]
            };
        }
        let mut out = Vec::new();
        let mut last = 0;
        for c in self.captures_all(s) {
            let (start, end) = (c.start(), c.end());
            if end == start && (start == 0 || start == s.len() || start == last) {
                continue;
            }
            out.push(&s[last..start]);
            last = end;
        }
        out.push(&s[last..]);
        out
    }

    /// Byte offset of the first match, as `s.search(re)`.
    pub fn search(&self, s: &str) -> Option<usize> {
        self.find(s).map(|r| r.start)
    }
}

/// A lazily compiled JS pattern: `static X: LazyRe = lazy_re!("a+", "g");`.
#[macro_export]
macro_rules! js_re {
    ($src:expr) => {
        std::sync::LazyLock::new(|| $crate::js::regex::Re::new($src, ""))
    };
    ($src:expr, $flags:expr) => {
        std::sync::LazyLock::new(|| $crate::js::regex::Re::new($src, $flags))
    };
}

pub type LazyRe = std::sync::LazyLock<Re>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_classes_and_boundaries() {
        assert!(Re::new(r"\bfoo\b", "").is_match("éfooé"));
        assert!(!Re::new(r"\w", "").is_match("é"));
        assert!(Re::new(r"(?<![\w.-])jq(?=$|\s)", "i").is_match("x JQ"));
        assert!(Re::new(r"\s", "").is_match("\u{FEFF}"));
        assert!(!Re::new(r"\s", "").is_match("\u{85}"));
        assert!(!Re::new(r"a.b", "").is_match("a\rb"));
        assert!(Re::new(r"(?<!x)\bfoo\b", "").is_match("éfooé"));
    }

    #[test]
    fn literal_braces_and_classes() {
        assert!(Re::new(r"\$\{?\w+\}?", "").is_match("${HOME}"));
        assert!(Re::new(r"[*?[]", "").is_match("a["));
        assert!(Re::new(r"a{2}", "").is_match("aa"));
        assert!(Re::new(r"x{", "").is_match("x{"));
    }

    #[test]
    fn multiline_breaks_at_cr() {
        let re = Re::new(r"^b$", "m");
        assert!(re.is_match("a\r\nb\r\nc"));
    }

    #[test]
    fn split_like_js() {
        let re = Re::new(r"\s*;\s*", "");
        assert_eq!(re.split("a ; b;c"), vec!["a", "b", "c"]);
        assert_eq!(Re::new(r"x", "").split(""), vec![""]);
    }
}
