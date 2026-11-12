//! String lengths and cuts in UTF-16 units, as JavaScript counts them.

/// `s.length` in JavaScript.
pub fn len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `s.slice(0, max)`. A cut through a surrogate pair leaves the lone high
/// half, which a browser draws as U+FFFD.
pub fn head(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        let n = c.len_utf16();
        if units + n > max {
            if units < max {
                return format!("{}\u{FFFD}", &s[..i]).into();
            }
            return s[..i].into();
        }
        units += n;
    }
    s.into()
}

/// `s` when it fits `max` units, else its first `max` units plus `…`.
pub fn clip(s: &str, max: usize) -> String {
    if len(s) > max {
        format!("{}…", head(s, max))
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_cuts_units() {
        assert_eq!(len("a😀"), 3);
        assert_eq!(head("a😀b", 2), "a\u{FFFD}");
        assert_eq!(head("a😀b", 3), "a😀");
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip("abc", 3), "abc");
    }
}
