//! JSONL text to rows, as every TS session parser reads it.

use rayon::prelude::*;

use crate::js::text;
use crate::js::value::JsValue;

const BOM: &str = "\u{FEFF}";

/// Lines above which rows are parsed in parallel.
const PARALLEL_LINES: usize = 256;

/// File bytes as `TextDecoder` reads them: a leading BOM dropped, bytes that
/// are not UTF-8 replaced by U+FFFD.
pub fn decode(bytes: Vec<u8>) -> String {
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    match text.strip_prefix(BOM) {
        Some(rest) => rest.to_owned(),
        None => text,
    }
}

/// Every line that parses as JSON, in file order. Blank and malformed lines
/// are skipped, so a row's position is not its line number.
pub fn parse_rows(text: &str) -> Vec<JsValue> {
    parse_rows_where(text, |_| true)
}

/// [`parse_rows`] over only the lines `keep` accepts.
pub fn parse_rows_where(text: &str, keep: impl Fn(&str) -> bool) -> Vec<JsValue> {
    let lines: Vec<&str> = text
        .split('\n')
        .filter(|l| !text::is_blank(l.as_bytes()) && keep(l))
        .collect();
    if lines.len() < PARALLEL_LINES {
        return lines.into_iter().filter_map(JsValue::parse).collect();
    }
    lines
        .into_par_iter()
        .map(JsValue::parse)
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect()
}

/// Every line that parses, plus the count of malformed lines, as
/// `scanJsonl` gives them. A bad last line is a live session's partial
/// write, not damage, so it is not counted.
pub fn scan_rows(text: &str) -> (Vec<JsValue>, u64) {
    let lines: Vec<&str> = text
        .split('\n')
        .filter(|l| !text::is_blank(l.as_bytes()))
        .collect();
    let parsed: Vec<Option<JsValue>> = if lines.len() < PARALLEL_LINES {
        lines.into_iter().map(JsValue::parse).collect()
    } else {
        lines.into_par_iter().map(JsValue::parse).collect()
    };
    let last_bad = parsed.last().is_some_and(Option::is_none);
    let bad = parsed.iter().filter(|p| p.is_none()).count() as u64;
    let rows = parsed.into_iter().flatten().collect();
    (rows, if last_bad { bad - 1 } else { bad })
}

/// Row `n` as [`parse_rows`] numbers it, parsing only that line: the `n`-th
/// line whose trimmed text starts with `{` and ends with `}`. `None` when
/// there is no such line or it does not parse.
pub fn line_at(text: &str, n: usize) -> Option<JsValue> {
    text.split('\n')
        .map(text::trim)
        .filter(|l| l.starts_with('{') && l.ends_with('}'))
        .nth(n)
        .and_then(JsValue::parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_skip_blank_and_bad() {
        let rows = parse_rows("{\"a\":1}\n\n  \nbad\nnull\r\n[1]\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], JsValue::Null);
    }

    #[test]
    fn scan_counts_bad_lines_but_the_last() {
        let (rows, bad) = scan_rows("x\n{\"a\":1}\n\ny\nnull\nz");
        assert_eq!((rows.len(), bad), (2, 2));
        assert_eq!(scan_rows("{}\n x \n").1, 0);
    }

    #[test]
    fn decodes_like_text_decoder() {
        assert_eq!(decode(b"\xEF\xBB\xBFab".to_vec()), "ab");
        assert_eq!(decode(b"a\xFFb".to_vec()), "a\u{FFFD}b");
    }

    #[test]
    fn line_at_counts_object_lines() {
        let text = "{\"a\":1}\nx\n {\"b\":2} \n{bad}";
        assert_eq!(
            line_at(text, 1).and_then(|v| v.get("b").cloned()),
            Some(JsValue::Num(2.0))
        );
        assert_eq!(line_at(text, 2), None);
        assert_eq!(line_at(text, 3), None);
    }
}
