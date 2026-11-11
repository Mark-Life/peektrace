//! Terminal width as `Bun.stringWidth` counts it, so `--pretty` columns line
//! up the same as in TS. `unicode-width` does the work; a per-char table and
//! the Lam-Alef rule correct the places where Bun disagrees.

use std::cmp::Ordering;

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

/// `(first, last, delta)`: `Bun.stringWidth` (Bun 1.4.4) minus `unicode-width`
/// (0.2.3) for a lone char in `first..=last`. Built by measuring every scalar
/// value in both; U+20E3 is left out because both agree on keycap sequences.
#[rustfmt::skip]
const BUN_DELTA: &[(u32, u32, i8)] = &[
    (0x5C8, 0x5C9, 1), (0x600, 0x604, -1), (0x6DD, 0x6DD, -1), (0x890, 0x891, 1),
    (0x93B, 0x93B, -1), (0x93E, 0x940, -1), (0x949, 0x94C, -1), (0x980, 0x980, -1),
    (0x982, 0x982, -1), (0x9BA, 0x9BB, -1), (0x9BF, 0x9C0, -1), (0x9C5, 0x9CC, -1),
    (0x9D1, 0x9D6, -1), (0xA00, 0xA00, -1), (0xA3A, 0xA3B, -1), (0xA3E, 0xA40, -1),
    (0xA43, 0xA46, -1), (0xA49, 0xA4A, -1), (0xA52, 0xA57, -1), (0xA62, 0xA63, -1),
    (0xA80, 0xA80, -1), (0xABA, 0xABB, -1), (0xABE, 0xAC0, -1), (0xAC6, 0xAC6, -1),
    (0xAC9, 0xACC, -1), (0xAD1, 0xAD7, -1), (0xB00, 0xB00, -1), (0xB02, 0xB02, -1),
    (0xB3A, 0xB3B, -1), (0xB40, 0xB40, -1), (0xB45, 0xB4C, -1), (0xB51, 0xB52, -1),
    (0xB80, 0xB81, -1), (0xBBA, 0xBBC, -1), (0xBBF, 0xBBF, -1), (0xBC1, 0xBCC, -1),
    (0xBD1, 0xBD6, -1), (0xBE2, 0xBE3, -1), (0xC01, 0xC02, -1), (0xC3A, 0xC3B, -1),
    (0xC41, 0xC45, -1), (0xC49, 0xC49, -1), (0xC51, 0xC54, -1), (0xC57, 0xC57, -1),
    (0xC80, 0xC80, -1), (0xC82, 0xC82, -1), (0xCBA, 0xCBB, -1), (0xCBE, 0xCBE, -1),
    (0xCC1, 0xCC1, -1), (0xCC3, 0xCC5, -1), (0xCC9, 0xCC9, -1), (0xCD1, 0xCD4, -1),
    (0xCD7, 0xCD7, -1), (0xD02, 0xD02, -1), (0xD3A, 0xD3A, -1), (0xD3F, 0xD40, -1),
    (0xD45, 0xD4C, -1), (0xD4E, 0xD4E, 1), (0xD57, 0xD57, 1), (0xDCF, 0xDCF, 1),
    (0xDDF, 0xDDF, 1), (0x1715, 0x1715, 1), (0x1734, 0x1734, 1), (0x17A4, 0x17A4, -1),
    (0x17D8, 0x17D8, -2), (0x1AF1, 0x1AFF, -1), (0x1B35, 0x1B35, 1), (0x1B3B, 0x1B3B, 1),
    (0x1B3D, 0x1B3D, 1), (0x1B43, 0x1B44, 1), (0x1BAA, 0x1BAA, 1), (0x1BF2, 0x1BF3, 1),
    (0x20F1, 0x20FF, -1), (0x2D7F, 0x2D7F, -1), (0x302E, 0x302F, 2), (0x3164, 0x3164, 2),
    (0xA8FA, 0xA8FA, 1), (0xA953, 0xA953, 1), (0xA9C0, 0xA9C0, 1), (0xD7C7, 0xD7CA, -1),
    (0xD7FC, 0xD7FF, -1), (0xFFA0, 0xFFA0, 1), (0xFFF0, 0xFFF8, 1), (0x10ECB, 0x10ECF, 1),
    (0x10EF0, 0x10EF9, 1), (0x111C0, 0x111C0, 1), (0x111C2, 0x111C3, 1), (0x11235, 0x11235, 1),
    (0x1133E, 0x1133E, 1), (0x1134D, 0x1134D, 1), (0x11357, 0x11357, 1), (0x113B8, 0x113B8, 1),
    (0x113C2, 0x113C2, 1), (0x113C5, 0x113C5, 1), (0x113C7, 0x113C9, 1), (0x113CF, 0x113CF, 1),
    (0x113D1, 0x113D1, 1), (0x114B0, 0x114B0, 1), (0x114BD, 0x114BD, 1), (0x115AF, 0x115AF, 1),
    (0x116B6, 0x116B6, 1), (0x11930, 0x11930, 1), (0x1193D, 0x1193D, 1), (0x1193F, 0x1193F, 1),
    (0x11941, 0x11941, 1), (0x11A84, 0x11A89, 1), (0x11D46, 0x11D46, 1), (0x11DF0, 0x11DF0, 1),
    (0x11F02, 0x11F02, 1), (0x11F41, 0x11F41, 1), (0x16FF0, 0x16FF1, 2), (0x18CD6, 0x18CDA, -1),
    (0x18D1F, 0x18D20, -1), (0x18E00, 0x19191, -1), (0x191A0, 0x191D2, -1),
    (0x1B123, 0x1B128, -1), (0x1B168, 0x1B168, -1), (0x1D127, 0x1D128, 1),
    (0x1D165, 0x1D166, 1), (0x1D16D, 0x1D172, 1), (0x1D250, 0x1D252, 1), (0x1D25B, 0x1D25C, 1),
    (0x1D25F, 0x1D25F, 1), (0x1D280, 0x1D281, 1), (0x1F1AE, 0x1F1AE, -1),
    (0x1F6D9, 0x1F6D9, -1), (0x1F7DA, 0x1F7DA, -1), (0x1FA8B, 0x1FA8D, -1),
    (0x1FACC, 0x1FACC, -1), (0x1FADD, 0x1FADD, -1), (0x1FAEB, 0x1FAEB, -1),
    (0x1FAF9, 0x1FAFA, -1), (0xE0080, 0xE00FF, 1), (0xE01F0, 0xE0FFF, 1),
];

/// Joining group Lam and Alef chars. `unicode-width` draws Lam then Alef as
/// one cell; Bun counts each.
const LAM: [char; 6] = [
    '\u{0644}', '\u{06B5}', '\u{06B6}', '\u{06B7}', '\u{06B8}', '\u{08A6}',
];
const ALEF: [char; 10] = [
    '\u{0622}', '\u{0623}', '\u{0625}', '\u{0627}', '\u{0671}', '\u{0672}', '\u{0673}', '\u{0675}',
    '\u{0773}', '\u{0774}',
];

fn bun_delta(c: char) -> isize {
    let cp = u32::from(c);
    BUN_DELTA
        .binary_search_by(|&(lo, hi, _)| {
            if hi < cp {
                Ordering::Less
            } else if lo > cp {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .map_or(0, |at| isize::from(BUN_DELTA[at].2))
}

/// Lam-Alef pairs `unicode-width` merged: a Lam, zero-width marks, an Alef.
fn lam_alef_pairs(s: &str) -> isize {
    let mut pending = false;
    let mut pairs = 0;
    for c in s.chars() {
        if pending && ALEF.contains(&c) {
            pairs += 1;
            pending = false;
        } else if LAM.contains(&c) {
            pending = true;
        } else if c.width() != Some(0) || matches!(c, '\u{200C}' | '\u{200D}') {
            pending = false;
        }
    }
    pairs
}

/// Width of text with no control chars.
fn visible_width(s: &str) -> usize {
    let base = s.width();
    if s.is_ascii() {
        return base;
    }
    let delta: isize = s.chars().map(bun_delta).sum::<isize>() + lam_alef_pairs(s);
    base.saturating_add_signed(delta)
}

/// Terminal columns `s` takes, as `Bun.stringWidth` counts them: ANSI CSI
/// sequences and control chars take none.
pub fn display_width(s: &str) -> usize {
    if !s.chars().any(char::is_control) {
        return visible_width(s);
    }
    let mut visible = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while chars
                .next()
                .is_some_and(|f| !('\u{40}'..='\u{7e}').contains(&f))
            {}
        } else if !c.is_control() {
            visible.push(c);
        }
    }
    visible_width(&visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_like_bun() {
        assert_eq!(display_width("日本語 🎉 title \u{1} \"q\" \\ end"), 26);
        assert_eq!(display_width("\u{1b}[31mred\u{1b}[0m"), 3);
        assert_eq!(display_width("\u{7f}\u{85}"), 0);
        assert_eq!(display_width("👨\u{200d}👩\u{200d}👧"), 2);
        assert_eq!(display_width("1\u{fe0f}\u{20e3}"), 2);
    }

    /// Expected values from `Bun.stringWidth` in Bun 1.4.4.
    #[test]
    fn scripts_unicode_width_counts_differently() {
        let cases = [
            ("a\u{0600}b", 2),
            ("\u{06DD}", 0),
            ("नमस्ते दुनिया", 8),
            ("हिंदी में शीर्षक", 9),
            ("তমিল", 3),
            ("தமிழ் தலைப்பு", 8),
            ("ગુજરાતી", 4),
            ("لا", 2),
            ("سلام لا بأس", 11),
            ("مرحبا بالعالم", 13),
            ("ภาษาไทย", 7),
        ];
        for (s, want) in cases {
            assert_eq!(display_width(s), want, "{s}");
        }
    }
}
