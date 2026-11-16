//! String lengths and cuts in UTF-16 units, as JavaScript counts them, over
//! `peektrace_js::units`. A cut through a surrogate pair shows as U+FFFD, as
//! a browser draws the lone half.

use peektrace_js::text::to_display;
use peektrace_js::units;

/// `s.length` in JavaScript.
pub fn len(s: &str) -> usize {
    units::len(s)
}

/// `s.length > max ? s.slice(0, max) + "…" : s`.
pub fn clip(s: &str, max: usize) -> String {
    let out = units::clip(s, max);
    match to_display(&out) {
        std::borrow::Cow::Borrowed(_) => out,
        std::borrow::Cow::Owned(shown) => shown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_cuts_units() {
        assert_eq!(len("a😀"), 3);
        assert_eq!(clip("a😀b", 2), "a\u{FFFD}…");
        assert_eq!(clip("a😀b", 3), "a😀…");
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip("abc", 3), "abc");
    }
}
