//! JS string operations on transcript text (see [`super::text::units`]),
//! measured in UTF-16 code units: `.length`, `.slice` and helpers built on
//! them. A cut through a surrogate pair leaves a lone surrogate, as in JS.

use super::text::{self, Unit};

/// True when `s` holds no stand-in or escaped char, so its chars are its
/// code points.
fn plain(s: &str) -> bool {
    !s.as_bytes().contains(&0xF4)
}

/// UTF-16 units of one decoded unit.
fn width(u: Unit) -> usize {
    match u {
        Unit::Char(c) => c.len_utf16(),
        Unit::Lone(_) => 1,
    }
}

/// `s.length`.
pub fn len(s: &str) -> usize {
    if plain(s) {
        // One unit per char, two for chars outside the BMP (4-byte UTF-8).
        return s
            .bytes()
            .map(|b| usize::from((b & 0xC0) != 0x80) + usize::from(b >= 0xF0))
            .sum();
    }
    text::units(s).map(width).sum()
}

/// Append one UTF-16 unit of a char that a cut split.
fn push_half(out: &mut String, unit: u16) {
    if let Some(c) = text::surrogate_stand_in(unit) {
        out.push(c);
    }
}

/// `s.slice(from, to)` for `from <= to`, both clamped to the length.
pub fn slice(s: &str, from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut at = 0;
    for u in text::units(s) {
        if at >= to {
            break;
        }
        let w = width(u);
        let end = at + w;
        if end > from {
            match u {
                Unit::Char(c) if at >= from && end <= to => text::push_literal(&mut out, c),
                Unit::Char(c) => {
                    let mut buf = [0u16; 2];
                    let pair = c.encode_utf16(&mut buf);
                    pair.iter()
                        .enumerate()
                        .filter(|(i, _)| (from..to).contains(&(at + i)))
                        .for_each(|(_, &unit)| push_half(&mut out, unit));
                }
                Unit::Lone(unit) => push_half(&mut out, unit),
            }
        }
        at = end;
    }
    out
}

/// The first `n` units of `s`, borrowed when nothing is cut.
pub fn head(s: &str, n: usize) -> std::borrow::Cow<'_, str> {
    if plain(s) && s.is_ascii() {
        return std::borrow::Cow::Borrowed(&s[..n.min(s.len())]);
    }
    if len(s) <= n {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(slice(s, 0, n))
}

/// `s.length > max ? s.slice(0, max) + "…" : s`.
pub fn clip(s: &str, max: usize) -> String {
    if len(s) > max {
        let mut out = slice(s, 0, max);
        out.push('…');
        out
    } else {
        s.to_owned()
    }
}

/// `Math.round(s.length / 4)` for non-empty `s`, else 0: the chars/4 token
/// estimate.
#[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
pub fn est_tokens(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    super::num::js_round(len(s) as f64 / 4.0)
}

/// First line (split on `\n`) whose trimmed text is not empty, trimmed and
/// cut to `cap` units plus `…`.
pub fn first_line(s: &str, cap: usize) -> String {
    let line = s
        .split('\n')
        .map(text::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    clip(line, cap)
}

/// The UTF-16 code units of transcript text.
pub fn to_units(s: &str) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for u in text::units(s) {
        match u {
            Unit::Char(c) => {
                let mut buf = [0u16; 2];
                out.extend_from_slice(c.encode_utf16(&mut buf));
            }
            Unit::Lone(unit) => out.push(unit),
        }
    }
    out
}

/// Transcript text from UTF-16 code units: unpaired surrogates become
/// stand-ins.
pub fn from_units(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    for r in char::decode_utf16(units.iter().copied()) {
        match r {
            Ok(c) => text::push_literal(&mut out, c),
            Err(e) => push_half(&mut out, e.unpaired_surrogate()),
        }
    }
    out
}

/// `s.replace(/\s+/g, " ")` with the JS `\s` set.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if text::is_js_whitespace(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(len("abc"), 3);
        assert_eq!(len("é😀"), 3);
        assert_eq!(len("a\u{10F800}"), 2);
        assert_eq!(len(""), 0);
    }

    #[test]
    fn slices_split_pairs() {
        let cut = slice("a😀b", 0, 2);
        assert_eq!(cut, "a\u{10F83D}");
        assert_eq!(text::to_display(&cut), "a\u{FFFD}");
        assert_eq!(slice("a😀b", 1, 3), "😀");
        assert_eq!(slice("abc", 1, 9), "bc");
    }

    #[test]
    fn unit_round_trip() {
        let s = "a😀\u{10F800}é";
        let u = to_units(s);
        assert_eq!(u, [0x61, 0xD83D, 0xDE00, 0xD800, 0xE9]);
        assert_eq!(from_units(&u), s);
        assert_eq!(from_units(&u[..2]), "a\u{10F83D}");
    }

    #[test]
    fn first_lines() {
        assert_eq!(first_line("\n  \n  hi there \nx", 200), "hi there");
        assert_eq!(first_line("", 200), "");
        assert_eq!(first_line("abcdef", 3), "abc…");
    }

    #[test]
    fn token_estimates() {
        assert_eq!(est_tokens(""), 0.0);
        assert_eq!(est_tokens("ab"), 1.0);
        assert_eq!(est_tokens("a"), 0.0);
        assert_eq!(est_tokens("abcdef"), 2.0);
    }
}
