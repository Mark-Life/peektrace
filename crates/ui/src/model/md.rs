//! Raw HTML in transcript Markdown. The kit drops HTML blocks and inline
//! tags, which would hide harness markers such as `<system-reminder>` and
//! their content. Escaping every HTML range shows them as text instead.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser};

/// `src` with the tag-opening `<` of every raw HTML range written as `&lt;`,
/// so it parses as text. Code spans, fences and autolinks outside HTML stay.
/// Uses the kit's parser options, so the ranges match what it would drop.
pub fn escape_html(src: &str) -> Cow<'_, str> {
    if !src.contains('<') {
        return Cow::Borrowed(src);
    }
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    // An HTML block comes as one event per line; joined, fences stay whole.
    let ranges = Parser::new_ext(src, opts)
        .into_offset_iter()
        .filter(|(ev, _)| matches!(ev, Event::Html(_) | Event::InlineHtml(_)))
        .map(|(_, r)| r)
        .fold(Vec::<Range<usize>>::new(), |mut acc, r| {
            match acc.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => acc.push(r),
            }
            acc
        });
    if ranges.is_empty() {
        return Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len() + 3 * ranges.len());
    let mut at = 0;
    for r in ranges {
        if r.start < at {
            continue;
        }
        out.push_str(&src[at..r.start]);
        escape_tags(&src[r.clone()], &mut out);
        at = r.end;
    }
    out.push_str(&src[at..]);
    Cow::Owned(out)
}

/// A block opened by a tag runs to the next blank line, so it can hold code
/// spans and fences: those keep their `<`, the entity would show verbatim.
fn escape_tags(html: &str, out: &mut String) {
    let mut fence: Option<(u8, usize)> = None;
    for line in html.split_inclusive('\n') {
        let mark = fence_mark(line);
        match (fence, mark) {
            (None, Some(m)) => {
                fence = Some(m);
                out.push_str(line);
            }
            (Some((c, n)), Some((mc, mn))) if c == mc && mn >= n => {
                fence = None;
                out.push_str(line);
            }
            (Some(_), _) => out.push_str(line),
            (None, None) => escape_line(line, out),
        }
    }
}

/// The fence char and run length when `line` opens or closes a fence.
fn fence_mark(line: &str) -> Option<(u8, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = *t.as_bytes().first()?;
    let n = t.bytes().take_while(|&b| b == c).count();
    ((c == b'`' || c == b'~') && n >= 3).then_some((c, n))
}

fn escape_line(line: &str, out: &mut String) {
    let b = line.as_bytes();
    let mut i = 0;
    let mut from = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                i += n;
                if let Some(end) = closing_ticks(&b[i..], n) {
                    i += end + n;
                }
            }
            b'<' if b.get(i + 1).is_some_and(|&c| opens_tag(c)) => {
                out.push_str(&line[from..i]);
                out.push_str("&lt;");
                i += 1;
                from = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&line[from..]);
}

/// Offset of the next run of exactly `n` backticks.
fn closing_ticks(b: &[u8], n: usize) -> Option<usize> {
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let run = b[i..].iter().take_while(|&&c| c == b'`').count();
            if run == n {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

const fn opens_tag(c: u8) -> bool {
    c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_but_not_code() {
        assert_eq!(escape_html("plain"), "plain");
        assert_eq!(escape_html("<x>\nhi\n</x>"), "&lt;x>\nhi\n&lt;/x>");
        assert_eq!(escape_html("a <b>c</b>"), "a &lt;b>c&lt;/b>");
        assert_eq!(
            escape_html("`<b>` and\n\n```\n<b>\n```"),
            "`<b>` and\n\n```\n<b>\n```"
        );
        assert_eq!(escape_html("<https://x.dev>"), "<https://x.dev>");
        assert_eq!(escape_html("1 < 2"), "1 < 2");
    }

    #[test]
    fn keeps_code_inside_an_html_block() {
        let src = "<context>\nUse `Option<T>` and x<y> here\n```rust\nlet v: Vec<u8> = a < b;\n```\n</context>";
        let out = escape_html(src);
        assert_eq!(
            out,
            "&lt;context>\nUse `Option<T>` and x&lt;y> here\n```rust\nlet v: Vec<u8> = a < b;\n```\n&lt;/context>"
        );
        let shown: String = Parser::new(&out)
            .filter_map(|ev| match ev {
                Event::Code(t) | Event::Text(t) => Some(t.into_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            shown,
            "<context>Use Option<T> and x<y> herelet v: Vec<u8> = a < b;\n</context>"
        );
    }
}
