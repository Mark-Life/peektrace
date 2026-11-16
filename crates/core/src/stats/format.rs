//! How stats text spells numbers: counts, durations, shares and tokens.

use crate::js::num::{js_round, parse_number, to_fixed, to_js_string};

/// `n.toLocaleString("en-US")` for integers: `1,234`. Fractions keep up to
/// three digits, as ICU rounds them.
pub fn count(n: f64) -> String {
    if !n.is_finite() {
        return to_js_string(n);
    }
    let neg = n < 0.0;
    let s = to_fixed(n.abs(), 3);
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let frac = frac.trim_end_matches('0');
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if neg && (grouped != "0" || !frac.is_empty()) {
        "-"
    } else {
        ""
    };
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

/// Duration as `1h04m`, `4m05s` or `2.3s`.
#[expect(clippy::cast_possible_truncation)]
pub fn hms(seconds: f64) -> String {
    let total = js_round(seconds).max(0.0);
    if total < 60.0 {
        return if seconds < 60.0 {
            format!("{}s", to_fixed(seconds, 1))
        } else {
            format!("{}s", to_js_string(total))
        };
    }
    let total = total as i64;
    if total < 3600 {
        return format!("{}m{:02}s", total / 60, total % 60);
    }
    format!("{}h{:02}m", total / 3600, (total % 3600) / 60)
}

/// Percentage from a 0..1 fraction: `12.5%`.
pub fn percent(fraction: f64) -> String {
    format!("{}%", to_fixed(fraction * 100.0, 1))
}

/// Compact token count: `950`, `12.3k`, `32.4M`.
pub fn tokens(value: f64) -> String {
    let n = js_round(value);
    if n < 1000.0 {
        to_js_string(n)
    } else if n < 1e6 {
        format!("{}k", to_fixed(n / 1000.0, 1))
    } else {
        format!("{}M", to_fixed(n / 1e6, 1))
    }
}

/// `String(Number(x.toPrecision(2)))` for `0.01 <= x` (smaller values print as 0 digits).
fn precision2(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return to_js_string(x);
    }
    let e = x.abs().log10().floor();
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let digits = (1.0 - e).max(0.0) as u32;
    to_js_string(parse_number(&to_fixed(x, digits)))
}

/// A per-1k rate: `<0.01` for a tiny nonzero rate, whole above 10, else 2 significant digits.
pub fn rate_text(rate: f64, hits: f64) -> String {
    if hits > 0.0 && rate < 0.01 {
        return "<0.01".to_owned();
    }
    if rate >= 10.0 {
        count(js_round(rate))
    } else {
        precision2(rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(count(1234.0), "1,234");
        assert_eq!(count(12.0), "12");
        assert_eq!(hms(59.4), "59.4s");
        assert_eq!(hms(59.94), "1m00s");
        assert_eq!(hms(125.0), "2m05s");
        assert_eq!(hms(3725.0), "1h02m");
        assert_eq!(tokens(12_345.0), "12.3k");
        assert_eq!(rate_text(0.125, 1.0), "0.13");
        assert_eq!(rate_text(3.33, 1.0), "3.3");
        assert_eq!(rate_text(0.0, 0.0), "0");
        assert_eq!(rate_text(0.001, 1.0), "<0.01");
        assert_eq!(rate_text(12.5, 1.0), "13");
    }
}
