//! Literal text as a case-sensitive JS pattern that matches what the same
//! text matches under a JS `i` flag without `u`: two UTF-16 units match when
//! their single-unit upper cases are equal, and a non-ASCII unit never maps
//! onto ASCII. Unicode case folding (the `regex` crate's `i`) also pairs the
//! Kelvin sign with `k`, long s with `s`, and astral letters; JS does not.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::stats::detectors::causes::escape_re;

/// `Canonicalize(ch)` of ECMA-262 for a non-`u` case-insensitive pattern.
fn canon(c: char) -> char {
    if u32::from(c) > 0xFFFF {
        return c;
    }
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) if (c.is_ascii() || !u.is_ascii()) && u32::from(u) <= 0xFFFF => u,
        _ => c,
    }
}

/// BMP chars grouped by the canonical char they fold to, for chars whose
/// canonical form is another char.
static FOLDS_TO: LazyLock<HashMap<char, Vec<char>>> = LazyLock::new(|| {
    let mut map: HashMap<char, Vec<char>> = HashMap::new();
    (0..=0xFFFF_u32)
        .filter_map(char::from_u32)
        .filter(|&c| canon(c) != c)
        .for_each(|c| map.entry(canon(c)).or_default().push(c));
    map
});

/// Every char that matches `c` case-insensitively, sorted.
fn equivalents(c: char) -> Vec<char> {
    let target = canon(c);
    let mut out = vec![c];
    if canon(target) == target {
        out.push(target);
    }
    if let Some(more) = FOLDS_TO.get(&target) {
        out.extend(more.iter().copied());
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `escapeRe(s)` with each cased char widened to a class of its JS
/// case-insensitive equivalents.
pub fn fold_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 4);
    for c in s.chars() {
        let eq = if c.is_ascii() && !c.is_ascii_alphabetic() {
            vec![c]
        } else {
            equivalents(c)
        };
        if let [only] = eq.as_slice() {
            out.push_str(&escape_re(only.encode_utf8(&mut [0; 4])));
        } else {
            out.push('[');
            out.extend(eq.iter());
            out.push(']');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::regex::Re;

    fn matches(q: &str, hay: &str) -> bool {
        Re::new(&fold_literal(q), "").is_match(hay)
    }

    #[test]
    fn folds_like_js() {
        assert!(matches("Hello", "hELLO"));
        assert!(matches("привет", "ПРИВЕТ"));
        assert!(matches("ΟΔΟΣ", "οδος"));
        assert!(matches("σ", "ς"));
        assert!(!matches("k", "\u{212A}"));
        assert!(!matches("s", "ſ"));
        assert!(!matches("ß", "ẞ"));
        assert!(!matches("i", "ı"));
        assert!(!matches("𐐀", "𐐨"));
        assert!(matches("a.b", "A.B"));
        assert!(!matches("a.b", "axb"));
    }

    #[test]
    fn specials_stay_escaped() {
        assert_eq!(fold_literal("1+$"), r"1\+\$");
        assert_eq!(fold_literal("k"), "[Kk]");
    }
}
