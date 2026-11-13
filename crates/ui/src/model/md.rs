//! Raw HTML in transcript Markdown. The kit drops HTML blocks and inline
//! tags, which would hide harness markers such as `<system-reminder>` and
//! their content. Escaping every HTML range shows them as text instead.

use std::borrow::Cow;

use pulldown_cmark::{Event, Options, Parser};

/// `src` with every raw HTML range's `<` written as `&lt;`, so it parses as
/// text. Code spans, fences and autolinks are not HTML ranges and stay.
/// Uses the kit's parser options, so the ranges match what it would drop.
pub fn escape_html(src: &str) -> Cow<'_, str> {
    if !src.contains('<') {
        return Cow::Borrowed(src);
    }
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let ranges: Vec<_> = Parser::new_ext(src, opts)
        .into_offset_iter()
        .filter(|(ev, _)| matches!(ev, Event::Html(_) | Event::InlineHtml(_)))
        .map(|(_, r)| r)
        .collect();
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
        out.push_str(&src[r.clone()].replace('<', "&lt;"));
        at = r.end;
    }
    out.push_str(&src[at..]);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_but_not_code() {
        assert_eq!(escape_html("plain"), "plain");
        assert_eq!(
            escape_html("<x>\nhi\n</x>"),
            "&lt;x>\nhi\n&lt;/x>"
        );
        assert_eq!(escape_html("a <b>c</b>"), "a &lt;b>c&lt;/b>");
        assert_eq!(escape_html("`<b>` and\n\n```\n<b>\n```"), "`<b>` and\n\n```\n<b>\n```");
        assert_eq!(escape_html("<https://x.dev>"), "<https://x.dev>");
        assert_eq!(escape_html("1 < 2"), "1 < 2");
    }
}
