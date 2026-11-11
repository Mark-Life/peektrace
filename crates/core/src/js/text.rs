//! String helpers that follow JavaScript semantics where output depends on them.

use std::cmp::Ordering;

/// `WhiteSpace` or `LineTerminator` as `String.prototype.trim` sees them.
pub const fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `String.prototype.trimEnd`.
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

/// True when `line.trim()` would be empty. Works on raw bytes so blank lines
/// skip UTF-8 decoding.
pub fn is_blank(line: &[u8]) -> bool {
    match line
        .iter()
        .position(|b| !matches!(b, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' '))
    {
        None => true,
        Some(at) if line[at].is_ascii() => false,
        Some(at) => String::from_utf8_lossy(&line[at..])
            .chars()
            .all(is_js_whitespace),
    }
}

/// First stand-in code point for an unpaired UTF-16 surrogate. Rust strings
/// cannot hold one, so U+D800..U+DFFF map onto the last 2048 code points of
/// plane 16 (private use) and map back when printed.
const STAND_IN_BASE: u32 = 0x10_F800;

/// The stand-in char for an unpaired surrogate `unit`.
pub fn surrogate_stand_in(unit: u16) -> Option<char> {
    (0xD800..0xE000)
        .contains(&unit)
        .then(|| char::from_u32(STAND_IN_BASE + u32::from(unit - 0xD800)))
        .flatten()
}

/// The unpaired surrogate a stand-in char represents.
pub fn stand_in_surrogate(c: char) -> Option<u16> {
    u32::from(c)
        .checked_sub(STAND_IN_BASE)
        .and_then(|d| u16::try_from(d).ok())
        .map(|d| 0xD800 + d)
}

/// Default `Array.prototype.sort` order: UTF-16 code units.
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Primary weight of an ASCII char in the CLDR root collation, or `None` for
/// chars the collation ignores (controls). Letters share a weight across case.
fn ascii_primary(b: u8) -> Option<u16> {
    const ORDER: &[u8] = b"\t\n\x0B\x0C\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789";
    if b.is_ascii_alphabetic() {
        let base = u16::try_from(ORDER.len()).ok()?;
        return Some(base + u16::from(b.to_ascii_lowercase() - b'a'));
    }
    ORDER
        .iter()
        .position(|&o| o == b)
        .and_then(|p| u16::try_from(p).ok())
}

/// Collation key of one char: primary weight, then case (lower first).
fn collation_key(c: char) -> Option<(u32, u8)> {
    if c.is_ascii() {
        let b = c as u8;
        return ascii_primary(b).map(|p| (u32::from(p), u8::from(b.is_ascii_uppercase())));
    }
    Some((0x1_0000 + u32::from(c), 0))
}

/// `String.prototype.localeCompare` under the root locale, for the ids that
/// reach it (ties on `updatedAt`). ASCII follows the CLDR root order
/// (punctuation, digits, then letters with lowercase first); other chars
/// sort after ASCII by code point. ICU's full collation is not reproduced.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let keys = |s: &str| s.chars().filter_map(collation_key).collect::<Vec<_>>();
    let (ka, kb) = (keys(a), keys(b));
    let primary = |k: &[(u32, u8)]| k.iter().map(|&(p, _)| p).collect::<Vec<_>>();
    primary(&ka)
        .cmp(&primary(&kb))
        .then_with(|| ka.iter().map(|k| k.1).cmp(kb.iter().map(|k| k.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_like_js() {
        assert_eq!(trim("\u{FEFF} a \u{00A0}"), "a");
        assert_eq!(trim("\u{0085}a"), "\u{0085}a");
        assert_eq!(trim_end("a \u{3000}"), "a");
    }

    #[test]
    fn blank_lines() {
        assert!(is_blank(b""));
        assert!(is_blank(b" \r\t"));
        assert!(is_blank("\u{00A0}\u{FEFF}".as_bytes()));
        assert!(!is_blank(b" {}"));
        assert!(!is_blank("\u{0085}".as_bytes()));
    }

    #[test]
    fn utf16_order_differs_from_code_points() {
        assert_eq!(cmp_utf16("\u{FFFF}", "\u{10000}"), Ordering::Greater);
        assert_eq!(cmp_utf16("a", "b"), Ordering::Less);
    }

    #[test]
    fn locale_compare_matches_icu_on_common_ids() {
        assert_eq!(locale_compare("a", "B"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("_x", "-x"), Ordering::Less);
        assert_eq!(locale_compare("1", "a"), Ordering::Less);
        assert_eq!(locale_compare("ab-1", "ab1"), Ordering::Less);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
    }
}
