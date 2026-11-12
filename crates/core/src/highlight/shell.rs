//! The shell tokenizer: heredoc bodies, quotes, `${}` and comments by
//! hand, then variables, keywords and command names.

use super::patterns::{self, Pattern, group, pat};
use super::script::prev_char;
use super::{Range, TokenClass as T};
use crate::js::regex::LazyRe;
use crate::js::text::is_js_whitespace;

static VARIABLE: LazyRe = crate::js_re!(r"\$[A-Za-z_][A-Za-z0-9_]*|\b[A-Z_][A-Z0-9_]*(?==)", "g");
static KEYWORD: LazyRe = crate::js_re!(
    r"\b(?:case|do|done|elif|else|esac|export|fi|for|function|if|in|select|then|time|until|while)\b",
    "g"
);
static COMMAND: LazyRe = crate::js_re!(r"(^|[\n;|&])\s*([A-Za-z0-9_./-]+)(?=\s|$)", "g");
static HEREDOC: LazyRe = crate::js_re!(r#"<<-?\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)\1"#, "g");

static PATTERNS: [Pattern; 3] = [
    pat(&VARIABLE, T::Variable),
    pat(&KEYWORD, T::Keyword),
    group(&COMMAND, 2, T::Command),
];

pub(super) fn collect(code: &str) -> Vec<Range> {
    patterns::collect_after(code, lexical(code), &PATTERNS)
}

fn lexical(code: &str) -> Vec<Range> {
    let heredocs = heredocs(code);
    let b = code.as_bytes();
    let mut ranges = heredocs.clone();
    let mut hi = 0;
    let mut i = 0;
    while i < b.len() {
        while hi < heredocs.len() && heredocs[hi].end <= i {
            hi += 1;
        }
        if let Some(h) = heredocs.get(hi)
            && h.start <= i
        {
            i = h.end;
            continue;
        }
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let mut end = i + 1;
            while end < b.len() {
                if b[end] == b'\\' && c == b'"' {
                    end += 2;
                } else if b[end] == c {
                    end += 1;
                    break;
                } else {
                    end += 1;
                }
            }
            let end = end.min(b.len());
            ranges.push(Range::new(i, end, T::String));
            i = end;
            continue;
        }
        if c == b'$' && b.get(i + 1) == Some(&b'{') {
            let end = code[i + 2..].find('}').map_or(b.len(), |p| i + 2 + p + 1);
            ranges.push(Range::new(i, end, T::Variable));
            i = end;
            continue;
        }
        if c == b'#'
            && prev_char(code, i)
                .is_none_or(|(_, p)| p == '\n' || is_js_whitespace(p) || ";|&()".contains(p))
        {
            let end = code[i..].find('\n').map_or(b.len(), |p| i + p);
            ranges.push(Range::new(i, end, T::Comment));
            i = end;
            continue;
        }
        i += 1;
    }
    ranges
}

fn heredocs(code: &str) -> Vec<Range> {
    let b = code.as_bytes();
    let mut ranges = Vec::new();
    for caps in HEREDOC.captures_all(code) {
        let at = caps.start();
        if (at > 0 && b[at - 1] == b'<') || inside_quote(b, at) {
            continue;
        }
        let Some(nl) = code[caps.end()..].find('\n') else {
            continue;
        };
        let body = caps.end() + nl + 1;
        let name = caps.get(2).unwrap_or("");
        let Some(close) = closing_line(&code[body..], name) else {
            ranges.push(Range::new(body, b.len(), T::String));
            continue;
        };
        let end = body + close.start;
        if end > body {
            ranges.push(Range::new(body, end, T::String));
        }
        ranges.push(Range::new(end, end + close.len(), T::Meta));
    }
    ranges
}

fn inside_quote(b: &[u8], index: usize) -> bool {
    let start = b[..index]
        .iter()
        .rposition(|c| *c == b'\n')
        .map_or(0, |p| p + 1);
    let mut quote = 0u8;
    let mut i = start;
    while i < index {
        let c = b[i];
        if c == b'\\' && quote != b'\'' {
            i += 1;
        } else if quote != 0 && c == quote {
            quote = 0;
        } else if quote == 0 && (c == b'"' || c == b'\'') {
            quote = c;
        }
        i += 1;
    }
    quote != 0
}

const fn is_line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// The first match of JS `/^\t*NAME\s*$/m` in `s`: line terminators are
/// `\n`, `\r`, U+2028 and U+2029, and `\s*` may run over them.
fn closing_line(s: &str, name: &str) -> Option<std::ops::Range<usize>> {
    let starts = std::iter::once(0).chain(
        s.char_indices()
            .filter(|(_, c)| is_line_end(*c))
            .map(|(i, c)| i + c.len_utf8()),
    );
    starts.into_iter().find_map(|start| {
        let rest = &s[start..];
        let after_tabs = rest.len() - rest.trim_start_matches('\t').len();
        let tail = rest[after_tabs..].strip_prefix(name)?;
        let ws_end = tail.len() - tail.trim_start_matches(is_js_whitespace).len();
        let base = start + after_tabs + name.len();
        let ws = &tail[..ws_end];
        let mut cuts: Vec<usize> = ws.char_indices().map(|(i, _)| i).collect();
        cuts.push(ws_end);
        cuts.into_iter().rev().find_map(|cut| {
            let next = tail[cut..].chars().next();
            next.is_none_or(is_line_end).then(|| start..base + cut)
        })
    })
}
