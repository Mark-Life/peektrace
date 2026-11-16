//! How stats text spells numbers: counts, durations, shares and tokens.

use crate::js::num::{js_round, to_fixed, to_js_string};
pub use crate::js::num::{rate_text, to_locale_string as count};

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

/// The inspector's spelling, for text the web and the native app show.
pub mod web {
    use crate::js::num::{js_round, to_fixed, to_js_string};

    fn pad2(s: String) -> String {
        if s.len() < 2 { format!("0{s}") } else { s }
    }

    /// `4h12m`, `3m07s`, `0.42s`. A minute rounds its seconds, so `119.6`
    /// is `1m60s`, as the inspector prints it.
    pub fn hms(sec: f64) -> String {
        if sec < 60.0 {
            let digits = if sec < 10.0 { 2 } else { 0 };
            return format!("{}s", to_fixed(sec, digits));
        }
        if sec < 3600.0 {
            let m = (sec / 60.0).floor();
            let s = js_round(sec - m * 60.0);
            return format!("{}m{}s", to_js_string(m), pad2(to_js_string(s)));
        }
        let h = (sec / 3600.0).floor();
        let m = ((sec - h * 3600.0) / 60.0).floor();
        format!("{}h{}m", to_js_string(h), pad2(to_js_string(m)))
    }

    /// A 0-1 share as a percentage with two decimals.
    pub fn pct(share: f64) -> String {
        format!("{}%", to_fixed(share * 100.0, 2))
    }

    /// Compact: `1.2K`, `1M`; whole thousands and millions drop the decimal.
    pub fn tokens(n: f64) -> String {
        let scaled = |unit: f64, suffix: &str| {
            let digits = u32::from(n % unit != 0.0);
            format!("{}{suffix}", to_fixed(n / unit, digits))
        };
        if n >= 1e6 {
            scaled(1e6, "M")
        } else if n >= 1000.0 {
            scaled(1000.0, "K")
        } else {
            to_js_string(js_round(n))
        }
    }
}

/// First segment of a UUID-like id, for narrow columns.
pub fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
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
        assert_eq!(web::hms(0.4), "0.40s");
        assert_eq!(web::hms(119.6), "1m60s");
        assert_eq!(web::hms(18277.0), "5h04m");
        assert_eq!(web::pct(0.157_1), "15.71%");
        assert_eq!(web::tokens(39_600.0), "39.6K");
        assert_eq!(web::tokens(2e6), "2M");
        assert_eq!(web::tokens(950.4), "950");
    }
}
