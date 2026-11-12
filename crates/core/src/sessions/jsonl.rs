//! JSONL text to rows, as every TS session parser reads it.

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

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

/// Lines per batch handed to the parser while streaming.
const BATCH_LINES: usize = 1024;

/// Bytes per batch handed to the parser while streaming.
const BATCH_BYTES: usize = 4 * 1024 * 1024;

const READ_BUFFER: usize = 64 * 1024;

/// Every line of `reader` split on `\n`, decoded as [`decode`] does, in
/// order, one at a time. True when the decoded text is empty.
pub fn read_lines(mut reader: impl BufRead, mut each: impl FnMut(&str)) -> io::Result<bool> {
    let mut buf = Vec::with_capacity(READ_BUFFER);
    let mut first = true;
    let mut empty = true;
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(empty);
        }
        let mut line = buf.as_slice();
        if first {
            line = line.strip_prefix(BOM.as_bytes()).unwrap_or(line);
            first = false;
        }
        empty &= line.is_empty();
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let text =
            std::str::from_utf8(line).map_or_else(|_| String::from_utf8_lossy(line), Cow::Borrowed);
        each(&text);
    }
}

/// Rows as `scanJsonl` yields them, fed one line at a time. Lines are
/// parsed in bounded batches, in parallel when a batch is large, so a big
/// file never sits in memory whole. A bad last line is a live session's
/// partial write, not damage, so it is not counted as malformed.
#[derive(Default)]
pub struct RowScan {
    /// The batch's lines back to back; `ends` marks where each stops.
    text: String,
    ends: Vec<usize>,
    bad: u64,
    last_bad: bool,
}

impl RowScan {
    /// Queue one raw line (no trailing `\n`); rows reach `each` in order.
    pub fn push(&mut self, line: &str, each: &mut impl FnMut(JsValue)) {
        if text::is_blank(line.as_bytes()) {
            return;
        }
        self.text.push_str(line);
        self.ends.push(self.text.len());
        if self.ends.len() >= BATCH_LINES || self.text.len() >= BATCH_BYTES {
            self.flush(each);
        }
    }

    fn flush(&mut self, each: &mut impl FnMut(JsValue)) {
        let starts = std::iter::once(0).chain(self.ends.iter().copied());
        let lines: Vec<&str> = starts
            .zip(&self.ends)
            .map(|(start, &end)| &self.text[start..end])
            .collect();
        let parsed: Vec<Option<JsValue>> = if lines.len() < PARALLEL_LINES {
            lines.into_iter().map(JsValue::parse).collect()
        } else {
            lines.into_par_iter().map(JsValue::parse).collect()
        };
        self.text.clear();
        self.ends.clear();
        for row in parsed {
            self.last_bad = row.is_none();
            match row {
                Some(v) => each(v),
                None => self.bad += 1,
            }
        }
    }

    /// Feed what is left to `each` and return the malformed line count.
    pub fn finish(mut self, each: &mut impl FnMut(JsValue)) -> u64 {
        self.flush(each);
        self.bad - u64::from(self.last_bad)
    }
}

/// A streamed scan's totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scanned {
    pub malformed: u64,
    /// The decoded text was empty.
    pub empty: bool,
}

/// Stream `reader`'s rows into `each` as [`RowScan`] yields them.
pub fn scan_reader(reader: impl BufRead, mut each: impl FnMut(JsValue)) -> io::Result<Scanned> {
    let mut scan = RowScan::default();
    let empty = read_lines(reader, |line| scan.push(line, &mut each))?;
    Ok(Scanned {
        malformed: scan.finish(&mut each),
        empty,
    })
}

/// Open `path` and stream its rows into `each`.
pub fn scan_file(path: &str, each: impl FnMut(JsValue)) -> io::Result<Scanned> {
    let file = File::open(path)?;
    scan_reader(BufReader::with_capacity(READ_BUFFER, file), each)
}

/// [`scan_reader`] over decoded text.
pub fn scan_text(text: &str, mut each: impl FnMut(JsValue)) -> u64 {
    let mut scan = RowScan::default();
    for line in text.split('\n') {
        scan.push(line, &mut each);
    }
    scan.finish(&mut each)
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

    fn scan(text: &str) -> (Vec<JsValue>, u64) {
        let mut rows = Vec::new();
        let bad = scan_text(text, |v| rows.push(v));
        (rows, bad)
    }

    fn scan_bytes(bytes: &[u8]) -> (Vec<JsValue>, Scanned) {
        let mut rows = Vec::new();
        let scanned = scan_reader(bytes, |v| rows.push(v)).expect("in-memory read");
        (rows, scanned)
    }

    #[test]
    fn scan_counts_bad_lines_but_the_last() {
        let (rows, bad) = scan("x\n{\"a\":1}\n\ny\nnull\nz");
        assert_eq!((rows.len(), bad), (2, 2));
        assert_eq!(scan("{}\n x \n").1, 0);
        assert_eq!(scan("").1, 0);
    }

    #[test]
    fn streamed_scan_matches_text_scan() {
        let text = "x\n{\"a\":1}\r\n\ny\nnull\nz\n";
        let (rows, scanned) = scan_bytes(text.as_bytes());
        assert_eq!((rows, scanned.malformed), scan(text));
        assert!(!scanned.empty);
        let (_, scanned) = scan_bytes(b"\xEF\xBB\xBF");
        assert!(scanned.empty);
        let (rows, _) = scan_bytes(b"\xEF\xBB\xBF{\"a\":\"\xFF\"}");
        assert_eq!(rows[0].get("a").and_then(JsValue::as_str), Some("\u{FFFD}"));
    }

    #[test]
    fn scan_crosses_batches_in_order() {
        let line = format!("{{\"s\":\"{}\"}}", "x".repeat(4096));
        let ids = (0_u32..).take(BATCH_LINES * 3 + 7);
        let lines: Vec<String> = ids
            .clone()
            .map(|i| {
                if i % 500 == 0 {
                    "bad".to_owned()
                } else {
                    line.replace('{', &format!("{{\"i\":{i},"))
                }
            })
            .collect();
        let (rows, scanned) = scan_bytes(lines.join("\n").as_bytes());
        let got: Vec<f64> = rows.iter().filter_map(|r| r.get("i")?.as_num()).collect();
        let want: Vec<f64> = ids.filter(|i| i % 500 != 0).map(f64::from).collect();
        assert_eq!(got, want);
        assert_eq!(scanned.malformed, 7);
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
