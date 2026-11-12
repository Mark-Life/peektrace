//! Pure formatters: numbers as the web inspector prints them, start times
//! read literally off the ISO string, and text made safe for a terminal.
//! Clipping counts display columns.

use peektrace_core::js::num::{js_round, to_fixed, to_js_string};
use peektrace_core::js::regex::LazyRe;
use peektrace_core::js::text;
use peektrace_core::stats::format::count;
use unicode_width::UnicodeWidthChar as _;

/// Thousands-separated rounded integer: `12,345`.
pub fn fmt(n: f64) -> String {
    count(js_round(n))
}

/// Compact magnitude: `12.3K`, `1.2M`.
pub fn fmt_k(n: f64) -> String {
    if n.abs() >= 1e6 {
        format!("{}M", to_fixed(n / 1e6, 1))
    } else if n.abs() >= 1e3 {
        format!("{}K", to_fixed(n / 1e3, 1))
    } else {
        to_js_string(js_round(n))
    }
}

/// Percentage from a 0..1 fraction: 1 decimal under 10%, integer above.
pub fn fmt_pct(fraction: f64) -> String {
    let pct = fraction * 100.0;
    if pct < 10.0 {
        format!("{}%", to_fixed(pct, 1))
    } else {
        format!("{}%", to_js_string(js_round(pct)))
    }
}

/// Byte size, 1 KB = 1024 B.
#[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
pub fn fmt_bytes(n: u64) -> String {
    let f = n as f64;
    if f < 1024.0 {
        format!("{n} B")
    } else if f < 1024.0 * 1024.0 {
        format!("{} KB", to_fixed(f / 1024.0, 1))
    } else {
        format!("{} MB", to_fixed(f / (1024.0 * 1024.0), 1))
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `2026-07-09T08:18…` -> `Jul 9, 08:18`; `—` when missing.
pub fn fmt_started(iso: Option<&str>) -> String {
    let Some(iso) = iso else {
        return "—".to_owned();
    };
    let b = iso.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let ok = digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10)
        && b.get(10) == Some(&b'T')
        && digits(11..13)
        && b.get(13) == Some(&b':')
        && digits(14..16);
    if !ok {
        return peektrace_core::js::units::slice(iso, 0, 16);
    }
    let month: usize = iso[5..7].parse().unwrap_or(0);
    let day: u32 = iso[8..10].parse().unwrap_or(0);
    let name = month
        .checked_sub(1)
        .and_then(|m| MONTHS.get(m))
        .map_or(&iso[5..7], |m| *m);
    format!("{name} {day}, {}", &iso[11..16])
}

static OSC: LazyRe = peektrace_core::js_re!(r"\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)", "g");
static CSI: LazyRe = peektrace_core::js_re!(r"\u001b\[[0-9;?]*[ -/]*[@-~]", "g");
static ESC: LazyRe = peektrace_core::js_re!(r"\u001b[@-Z\\-_]", "g");

/// Strip escape sequences and control bytes: OSC, CSI and other ESC
/// sequences, CR/CRLF to LF, tab to two spaces, C0 controls but LF.
pub fn sanitize(s: &str) -> String {
    if !s.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return s.to_owned();
    }
    let s = OSC.replace_all_str(s, "");
    let s = CSI.replace_all_str(&s, "");
    let s = ESC.replace_all_str(&s, "");
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let s = s.replace('\t', "  ");
    s.chars()
        .filter(|c| !matches!(*c as u32, 0..=8 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f))
        .collect()
}

/// Display columns of one char (control chars count 0).
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// Display columns of `s`.
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// The longest prefix of `s` that fits `max` columns.
pub fn take_width(s: &str, max: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        let w = char_width(c);
        if used + w > max {
            return &s[..i];
        }
        used += w;
    }
    s
}

/// Truncate to `max` columns with a trailing `…` when clipped.
pub fn clip(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_owned();
    }
    format!("{}…", take_width(s, max.saturating_sub(1)))
}

/// First line, sanitized and trimmed, clipped to `max` columns.
pub fn first_line(s: &str, max: usize) -> String {
    let clean = sanitize(s.split_once('\n').map_or(s, |(a, _)| a));
    clip(text::trim(&clean), max)
}

/// A bar of `width` cells filled to `fraction`.
pub fn bar_glyphs(fraction: f64, width: usize) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "clamped to 0..=width"
    )]
    let filled = js_round(fraction * width as f64).clamp(0.0, width as f64) as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Pad with spaces on the right to `w` columns.
pub fn pad_end(s: &str, w: usize) -> String {
    let n = width(s);
    if n >= w {
        s.to_owned()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

/// Pad with spaces on the left to `w` columns.
pub fn pad_start(s: &str, w: usize) -> String {
    let n = width(s);
    if n >= w {
        s.to_owned()
    } else {
        format!("{}{s}", " ".repeat(w - n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(fmt(12_345.4), "12,345");
        assert_eq!(fmt(0.5), "1");
        assert_eq!(fmt_k(12_345.0), "12.3K");
        assert_eq!(fmt_k(1_200_000.0), "1.2M");
        assert_eq!(fmt_k(999.0), "999");
        assert_eq!(fmt_pct(0.0734), "7.3%");
        assert_eq!(fmt_pct(0.22), "22%");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1638), "1.6 KB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn started() {
        assert_eq!(fmt_started(Some("2026-07-09T08:18:00Z")), "Jul 9, 08:18");
        assert_eq!(fmt_started(None), "—");
        assert_eq!(fmt_started(Some("yesterday at noon")), "yesterday at noo");
    }

    #[test]
    fn sanitize_strips_terminal_sequences() {
        assert_eq!(sanitize("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(
            sanitize("a\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{1b}\\b"),
            "alinkb"
        );
        assert_eq!(sanitize("x\u{1b}Ky"), "xy");
        assert_eq!(sanitize("a\r\nb\rc"), "a\nb\nc");
        assert_eq!(sanitize("a\tb"), "a  b");
        assert_eq!(sanitize("a\u{0}\u{7}\u{7f}b\nc"), "ab\nc");
    }

    #[test]
    fn clipping_counts_columns() {
        assert_eq!(clip("hello", 5), "hello");
        assert_eq!(clip("hello!", 5), "hell…");
        assert_eq!(clip("日本語です", 5), "日本…");
        assert_eq!(first_line("  one\ntwo", 120), "one");
        assert_eq!(bar_glyphs(0.5, 4), "██░░");
    }
}
