//! Release version order, by semver precedence.

use std::cmp::Ordering;

use super::release::TAG_PREFIX;

/// `parseInt(s, 10)`, with no digits read as 0.
fn leading_int(s: &str) -> f64 {
    let s = s.trim_start();
    let (sign, digits) = match s.as_bytes().first() {
        Some(b'-') => (-1.0, &s[1..]),
        Some(b'+') => (1.0, &s[1..]),
        _ => (1.0, s),
    };
    let end = digits
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse::<f64>().map_or(0.0, |n| sign * n)
}

/// The version without the tag prefix or a leading `v`.
fn strip_prefix(v: &str) -> &str {
    v.strip_prefix(TAG_PREFIX)
        .or_else(|| v.strip_prefix('v'))
        .unwrap_or(v)
}

/// The dotted core and the pre-release part; `+build` is dropped.
fn split(v: &str) -> (&str, Option<&str>) {
    let v = strip_prefix(v);
    let v = v.split_once('+').map_or(v, |(head, _)| head);
    match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    }
}

/// Dotted numeric cores; a missing part reads as 0.
fn compare_cores(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| v.split('.').map(leading_int).collect::<Vec<_>>();
    let (pa, pb) = (parts(a), parts(b));
    (0..pa.len().max(pb.len()))
        .map(|i| {
            let x = pa.get(i).copied().unwrap_or(0.0);
            let y = pb.get(i).copied().unwrap_or(0.0);
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        })
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// One pre-release identifier: numeric ones by value and below text ones.
fn compare_ident(a: &str, b: &str) -> Ordering {
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    match (numeric(a), numeric(b)) {
        (true, true) => {
            let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
            a.len().cmp(&b.len()).then_with(|| a.cmp(b))
        }
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

/// Pre-release parts; none sorts above any, a shorter equal prefix below.
fn compare_pre(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let (ia, ib) = (a.split('.'), b.split('.'));
            ia.clone()
                .zip(ib.clone())
                .map(|(x, y)| compare_ident(x, y))
                .find(|o| o.is_ne())
                .unwrap_or_else(|| ia.count().cmp(&ib.count()))
        }
    }
}

/// Semver order, ignoring the tag prefix: `0.13.0-rc.1` < `0.13.0`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let ((ca, pa), (cb, pb)) = (split(a), split(b));
    compare_cores(ca, cb).then_with(|| compare_pre(pa, pb))
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering::{Equal, Greater, Less};

    use super::*;

    #[test]
    fn versions() {
        let cases = [
            ("cli-v9.9.9", "0.0.0-dev", Greater),
            ("cli-v0.0.0", "0.0.0-dev", Greater),
            ("v1.2", "1.2.0", Equal),
            ("1.10.0", "1.9.9", Greater),
            ("cli-v0.11.0", "cli-v0.12.0", Less),
            ("x", "0", Equal),
            ("cli-v9.9.9", "0.0.1", Greater),
            ("1.2.3", "1.2.3", Equal),
            ("v1.0.0", "v1.2.0", Less),
            ("0.0.0-dev", "cli-v0.0.1", Less),
            ("0.0.0-dev", "cli-v0.12.0", Less),
            ("cli-v0.13.0", "0.13.0-rc.1", Greater),
            ("cli-v0.13.0", "0.13.0-dev", Greater),
            ("cli-v1.0.0-rc.1", "1.0.0-beta.2", Greater),
            ("1.0.0-alpha", "1.0.0-alpha.1", Less),
            ("1.0.0-alpha.1", "1.0.0-alpha.beta", Less),
            ("1.0.0-beta.2", "1.0.0-beta.11", Less),
            ("1.0.0-rc.1", "1.0.0", Less),
            ("1.0.0+build.5", "1.0.0", Equal),
            ("0.13.0", "cli-v0.13.0", Equal),
            ("0.13.0", "v0.13.0", Equal),
        ];
        for (a, b, want) in cases {
            assert_eq!(compare_versions(a, b), want, "{a} vs {b}");
            assert_eq!(compare_versions(b, a), want.reverse(), "{b} vs {a}");
        }
    }

    #[test]
    fn leading_ints() {
        assert!((leading_int("0") - 0.0).abs() < f64::EPSILON);
        assert!((leading_int("12abc") - 12.0).abs() < f64::EPSILON);
        assert!((leading_int(" -3") + 3.0).abs() < f64::EPSILON);
    }
}
