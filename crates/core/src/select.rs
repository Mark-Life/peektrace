//! Client-side selection for `sessions ls`: `--since`, `--cwd`, newest-first
//! sort and `--limit`.

use std::cmp::Ordering;

use crate::js::{num, text, time};
use crate::model::SessionHeader;
use crate::paths;

const MAX_DATE_MS: f64 = 8.64e15;

/// `updatedAt` as epoch ms, or `None` when it does not parse.
fn time_of(h: &SessionHeader) -> Option<i64> {
    time::parse_date(&h.updated_at)
}

/// Newest first; rows with no parseable time last; ties broken by id.
fn by_updated_desc(a: &SessionHeader, b: &SessionHeader) -> Ordering {
    match (time_of(a), time_of(b)) {
        (ta, tb) if ta == tb => text::locale_compare(a.id.as_str(), b.id.as_str()),
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(ta), Some(tb)) => tb.cmp(&ta),
    }
}

/// `YYYY-MM-DD[(T| )time[zone]]` where time is `[\d:.]+` and zone is `Z` or
/// `±HH[:]MM`.
fn looks_iso(s: &str) -> bool {
    let b = s.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let date = b.len() >= 10
        && [0, 1, 2, 3, 5, 6, 8, 9].into_iter().all(digit)
        && b[4] == b'-'
        && b[7] == b'-';
    if !date {
        return false;
    }
    let Some(rest) = b.get(10..).filter(|r| !r.is_empty()) else {
        return true;
    };
    if !matches!(rest[0], b'T' | b' ') {
        return false;
    }
    let run = rest[1..]
        .iter()
        .take_while(|c| c.is_ascii_digit() || matches!(c, b':' | b'.'))
        .count();
    run > 0
        && (rest[1 + run..].is_empty()
            || zone_suffix_len(&rest[1 + run..]) == Some(rest.len() - 1 - run))
}

/// Length of a `Z` or `±HH[:]MM` zone that ends `s`.
fn zone_suffix_len(s: &[u8]) -> Option<usize> {
    if s.ends_with(b"Z") {
        return Some(1);
    }
    let digits = |x: &[u8]| x.iter().all(u8::is_ascii_digit);
    [6, 5].into_iter().find(|&n| {
        s.len() >= n && {
            let z = &s[s.len() - n..];
            matches!(z[0], b'+' | b'-')
                && if n == 6 {
                    digits(&z[1..3]) && z[3] == b':' && digits(&z[4..])
                } else {
                    digits(&z[1..])
                }
        }
    })
}

/// True when the leading `YYYY-MM-DD` is a real calendar day.
fn is_real_day(s: &str) -> bool {
    let day = &s[..10];
    time::parse_date(&format!("{day}T00:00:00Z"))
        .is_some_and(|ms| time::to_iso_string(ms).get(..10) == Some(day))
}

/// ISO input as epoch ms; a time with no offset is UTC.
fn parse_iso(s: &str) -> Option<f64> {
    if !(looks_iso(s) && is_real_day(s)) {
        return None;
    }
    let iso = s.replacen(' ', "T", 1);
    let iso = if iso.contains('T') && zone_suffix_len(iso.as_bytes()).is_none() {
        format!("{iso}Z")
    } else {
        iso
    };
    #[expect(clippy::cast_precision_loss, reason = "dates are within 8.64e15")]
    time::parse_date(&iso).map(|ms| ms as f64)
}

/// `--since`: a relative value (`7d`, `12h`, `2w`) or ISO date, as epoch ms.
/// `None` when invalid.
pub fn parse_since(input: &str, now_ms: f64) -> Option<f64> {
    let t = text::trim(input);
    let relative = t
        .char_indices()
        .last()
        .and_then(|(at, unit)| {
            let n = &t[..at];
            let ms = match unit {
                'h' => 3_600_000.0,
                'd' => 86_400_000.0,
                'w' => 604_800_000.0,
                _ => return None,
            };
            (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some((n, ms))
        })
        .map(|(n, ms)| now_ms - num::parse_number(n) * ms);
    let parsed = relative.or_else(|| parse_iso(t))?;
    (parsed.is_finite() && parsed.abs() <= MAX_DATE_MS).then_some(parsed)
}

/// Drop trailing `/` and `\`; empty becomes `/`.
fn strip_separators(path: &str) -> &str {
    let s = path.trim_end_matches(['/', '\\']);
    if s.is_empty() { "/" } else { s }
}

/// True when resolving `input` as `--cwd` reads the working directory.
pub fn needs_base_dir(input: &str) -> bool {
    !(input == "~" || input.starts_with("~/") || input.starts_with('/'))
}

/// Resolve a `--cwd` argument against `cwd`, expanding a leading `~`.
pub fn resolve_cwd(input: &str, home: &str, cwd: &str) -> String {
    let expanded = if input == "~" || input.starts_with("~/") {
        format!("{home}{}", &input[1..])
    } else {
        input.to_owned()
    };
    strip_separators(&paths::resolve(cwd, &expanded)).to_owned()
}

fn is_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') || p.starts_with("\\\\")
}

/// True when `have` is `want` or inside it. Both `/` and `\` separate;
/// Windows paths compare case-insensitively.
fn matches_cwd(have: Option<&str>, want: &str) -> bool {
    let Some(have) = have else {
        return false;
    };
    let fold = |p: &str| {
        if is_windows_path(want) {
            p.to_lowercase().replace('/', "\\")
        } else {
            p.to_owned()
        }
    };
    let have = fold(strip_separators(have));
    let want = fold(strip_separators(want));
    if want == "/" || have == want {
        return true;
    }
    have.starts_with(&want) && matches!(have.as_bytes().get(want.len()), Some(b'/' | b'\\'))
}

/// `sessions ls` selection options.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectOptions {
    pub since_ms: Option<f64>,
    pub cwd: Option<String>,
    /// Max rows; 0 means all.
    pub limit: usize,
}

/// Rows to show and how many the limit cut.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub rows: Vec<SessionHeader>,
    pub hidden: usize,
}

/// Apply `--since`, `--cwd`, sort, then `--limit`.
pub fn select_sessions(headers: Vec<SessionHeader>, opts: &SelectOptions) -> Selection {
    #[expect(clippy::cast_precision_loss, reason = "dates are within 8.64e15")]
    let mut kept: Vec<_> = headers
        .into_iter()
        .filter(|h| {
            opts.since_ms
                .is_none_or(|since| time_of(h).is_some_and(|t| t as f64 >= since))
        })
        .filter(|h| {
            opts.cwd
                .as_deref()
                .is_none_or(|want| matches_cwd(h.cwd.as_deref(), want))
        })
        .collect();
    kept.sort_by(by_updated_desc);
    let total = kept.len();
    if opts.limit > 0 {
        kept.truncate(opts.limit);
    }
    let hidden = total - kept.len();
    Selection { rows: kept, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_794_415_171_257.0;

    #[test]
    fn since_relative() {
        assert_eq!(parse_since("7d", NOW), Some(NOW - 7.0 * 86_400_000.0));
        assert_eq!(parse_since(" 0d ", NOW), Some(NOW));
        assert_eq!(parse_since("2w", NOW), Some(NOW - 2.0 * 604_800_000.0));
        assert_eq!(parse_since("7D", NOW), None);
        assert_eq!(parse_since("7m", NOW), None);
        assert_eq!(parse_since("d", NOW), None);
        assert_eq!(parse_since("99999999999999w", NOW), None);
    }

    #[test]
    fn since_iso() {
        assert_eq!(parse_since("2026-01-31", NOW), Some(1_769_817_600_000.0));
        assert_eq!(parse_since("2026-01-31T10", NOW), None);
        assert_eq!(
            parse_since("2026-01-31T10:00", NOW),
            Some(1_769_853_600_000.0)
        );
        assert_eq!(
            parse_since("2026-01-31 10:00:00", NOW),
            Some(1_769_853_600_000.0)
        );
        assert_eq!(
            parse_since("2026-01-31T10:00:00.123456", NOW),
            Some(1_769_853_600_123.0)
        );
        assert_eq!(
            parse_since("2026-01-31T10:00+0100", NOW),
            Some(1_769_850_000_000.0)
        );
        assert_eq!(
            parse_since("2026-01-31T10:00+01:00", NOW),
            Some(1_769_850_000_000.0)
        );
        assert_eq!(
            parse_since("2026-01-31T24:00", NOW),
            Some(1_769_904_000_000.0)
        );
        for bad in [
            "2026-01-31T25:00",
            "2026-01-31T10:60",
            "2026-01-31T1:2",
            "2026-01-31T10:00:00:00",
            "2026-01-31Z",
            "2026-02-30",
            "xx",
            "2026-1-31",
        ] {
            assert_eq!(parse_since(bad, NOW), None, "{bad}");
        }
    }

    #[test]
    fn cwd_resolution_and_match() {
        assert_eq!(resolve_cwd("~/x/", "/home/u", "/w"), "/home/u/x");
        assert_eq!(resolve_cwd("a/../b", "/home/u", "/w"), "/w/b");
        assert_eq!(resolve_cwd("/", "/home/u", "/w"), "/");
        assert!(matches_cwd(Some("/a/b/"), "/a/b"));
        assert!(matches_cwd(Some("/a/b/c"), "/a/b"));
        assert!(!matches_cwd(Some("/a/bc"), "/a/b"));
        assert!(matches_cwd(Some("/x"), "/"));
        assert!(!matches_cwd(None, "/"));
        assert!(matches_cwd(Some("C:/Users/A/proj"), "c:\\users\\a"));
    }
}
