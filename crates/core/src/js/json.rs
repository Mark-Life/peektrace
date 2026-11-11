//! Rewrite JSON text that `JSON.parse` accepts but `serde_json` rejects:
//! unpaired surrogate escapes and numbers beyond the `f64` range.

use super::text;

/// What an out-of-range number literal becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    /// A string [`overflow_value`] reads back as `±Infinity`.
    Sentinel,
    /// `0`, for callers that only check a value's type.
    Zero,
}

/// Sentinel strings for `+Infinity` and `-Infinity`. `ESCAPE` before a
/// non-reserved char never occurs in transcript text, so no real string in a
/// repaired line can equal them.
const POS_INF: &str = "\u{10F7FF}+";
const NEG_INF: &str = "\u{10F7FF}-";

/// `±Infinity` when `s` is an overflow sentinel. Only meaningful for strings
/// parsed from [`repair`] output.
pub fn overflow_value(s: &str) -> Option<f64> {
    if s.len() != POS_INF.len() {
        return None;
    }
    match s {
        POS_INF => Some(f64::INFINITY),
        NEG_INF => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

/// The UTF-16 unit of the `\uXXXX` escape at `at`, if there is one.
fn escape_at(b: &[u8], at: usize) -> Option<u16> {
    let hex = b.get(at..at + 6)?.strip_prefix(b"\\u")?;
    let hex = std::str::from_utf8(hex).ok()?;
    u16::from_str_radix(hex, 16).ok()
}

/// The code point a `\\uHIGH\\uLOW` escape pair at `at` encodes.
fn pair_at(b: &[u8], at: usize) -> Option<char> {
    let high = escape_at(b, at).filter(|u| (0xD800..0xDC00).contains(u))?;
    let low = escape_at(b, at + 6).filter(|u| (0xDC00..0xE000).contains(u))?;
    char::decode_utf16([high, low]).next()?.ok()
}

/// Length of the JSON number token at the start of `b`, or 0 when `b` does
/// not start with one.
fn number_len(b: &[u8]) -> usize {
    let digits = |from: usize| {
        b[from.min(b.len())..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let mut at = usize::from(b.first() == Some(&b'-'));
    match b.get(at) {
        Some(b'0') => at += 1,
        Some(b'1'..=b'9') => at += digits(at),
        _ => return 0,
    }
    if b.get(at) == Some(&b'.') {
        let frac = digits(at + 1);
        if frac == 0 {
            return 0;
        }
        at += 1 + frac;
    }
    if matches!(b.get(at), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(at + 1), Some(b'+' | b'-')));
        let exp = digits(at + 1 + sign);
        if exp == 0 {
            return 0;
        }
        at += 1 + sign + exp;
    }
    at
}

/// The replacement for an out-of-range number token, or `None` when it fits.
fn overflow_token(token: &str, mode: Overflow) -> Option<String> {
    let value = token.parse::<f64>().ok().filter(|v| v.is_infinite())?;
    Some(match mode {
        Overflow::Zero => "0".to_owned(),
        Overflow::Sentinel if value > 0.0 => format!("\"{POS_INF}\""),
        Overflow::Sentinel => format!("\"{NEG_INF}\""),
    })
}

/// `text` with each unpaired surrogate escape (`\\ud800`) replaced by its
/// stand-in char and each number beyond the `f64` range replaced per `mode`,
/// or `None` when there is neither. Strings parsed from the result are
/// already transcript text: real chars that could pass for a stand-in are
/// escaped too.
pub fn repair(text: &str, mode: Overflow) -> Option<String> {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut in_string = false;
    let mut copied = 0;
    let mut at = 0;
    while at < b.len() {
        if !in_string && (b[at] == b'-' || b[at].is_ascii_digit()) {
            let len = number_len(&b[at..]).max(1);
            if let Some(token) = text.get(at..at + len)
                && let Some(fixed) = overflow_token(token, mode)
            {
                out.push_str(&text[copied..at]);
                out.push_str(&fixed);
                changed = true;
                copied = at + len;
            }
            at += len;
            continue;
        }
        if b[at] == b'"' {
            in_string = !in_string;
            at += 1;
            continue;
        }
        // A reserved char, raw (UTF-8 lead byte 0xF4) or as an escape pair.
        let literal = match b[at] {
            0xF4 => text[at..].chars().next().map(|c| (c, c.len_utf8())),
            b'\\' => pair_at(b, at).map(|c| (c, 12)),
            _ => None,
        };
        if let Some((c, len)) = literal {
            if text::reserved(c) {
                out.push_str(&text[copied..at]);
                text::push_literal(&mut out, c);
                copied = at + len;
            }
            at += len;
            continue;
        }
        if b[at] != b'\\' {
            at += 1;
            continue;
        }
        match escape_at(b, at).map(text::surrogate_stand_in) {
            None => at += 2,
            Some(Some(stand_in)) => {
                out.push_str(&text[copied..at]);
                out.push(stand_in);
                changed = true;
                at += 6;
                copied = at;
            }
            Some(None) => at += 6,
        }
    }
    out.push_str(&text[copied..]);
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_chars_escape_beside_lone_surrogates() {
        let raw = "{\"a\":\"\\ud801\u{10F801}\\udbfe\\udc01\\ud83d\\ude00\"}";
        assert_eq!(
            repair(raw, Overflow::Sentinel).as_deref(),
            Some("{\"a\":\"\u{10F801}\u{10F7FF}\u{10F801}\u{10F7FF}\u{10F801}\\ud83d\\ude00\"}")
        );
    }

    #[test]
    fn lone_surrogates() {
        assert_eq!(
            repair(
                r#"{"a":"\ud800","b":"\\ud800","c":"\ud83d\ude00\udc00"}"#,
                Overflow::Sentinel
            )
            .as_deref(),
            Some("{\"a\":\"\u{10F800}\",\"b\":\"\\\\ud800\",\"c\":\"\\ud83d\\ude00\u{10FC00}\"}")
        );
        assert_eq!(repair(r#"{"a":"\u0041"}"#, Overflow::Sentinel), None);
    }

    #[test]
    fn out_of_range_numbers() {
        let raw = r#"{"a":1e400,"b":[-1E+309,1e-400,12.5e3],"c":"1e400","d":"\"1e400"}"#;
        assert_eq!(
            repair(raw, Overflow::Sentinel).as_deref(),
            Some(
                "{\"a\":\"\u{10F7FF}+\",\"b\":[\"\u{10F7FF}-\",1e-400,12.5e3],\"c\":\"1e400\",\"d\":\"\\\"1e400\"}"
            )
        );
        assert_eq!(
            repair(raw, Overflow::Zero).as_deref(),
            Some(r#"{"a":0,"b":[0,1e-400,12.5e3],"c":"1e400","d":"\"1e400"}"#)
        );
        let long = format!("[{}]", "9".repeat(400));
        assert_eq!(repair(&long, Overflow::Zero).as_deref(), Some("[0]"));
        assert_eq!(repair(r#"{"a":1.5,"b":-0}"#, Overflow::Zero), None);
    }

    #[test]
    fn sentinels_read_back() {
        assert_eq!(overflow_value(POS_INF), Some(f64::INFINITY));
        assert_eq!(overflow_value(NEG_INF), Some(f64::NEG_INFINITY));
        assert_eq!(overflow_value("\u{10F7FF}\u{10F7FF}+"), None);
        assert_eq!(overflow_value("+"), None);
    }
}
