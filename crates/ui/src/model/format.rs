//! Number, size and health formatting shared by the session views, with the
//! JavaScript rounding the web inspector uses.

use super::jsnum::{js_round, to_fixed, to_js_string};

const MILLION: f64 = 1_000_000.0;
const THOUSAND: f64 = 1000.0;
const WARN_THRESHOLD: f64 = 0.75;

/// A coarse health zone: context fill against the dumb-zone threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    Ok,
    Warn,
    Bad,
}

impl Zone {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Bad => "bad",
        }
    }

    /// The verdict word.
    pub const fn health(self) -> &'static str {
        match self {
            Self::Ok => "Healthy",
            Self::Warn => "Degrading",
            Self::Bad => "Rotting",
        }
    }
}

/// The zone of a fill fraction. The warn edge sits at 0.75, or halfway from
/// a higher dumb-zone threshold to 1, so the warn band never collapses.
pub fn zone_of(frac: f64, dumb_zone: f64) -> Zone {
    let warn = if dumb_zone < WARN_THRESHOLD {
        WARN_THRESHOLD
    } else {
        f64::midpoint(1.0, dumb_zone)
    };
    if frac < dumb_zone {
        Zone::Ok
    } else if frac < warn {
        Zone::Warn
    } else {
        Zone::Bad
    }
}

/// `n.toLocaleString("en-US")` for an integral `n`.
fn grouped(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "∞" } else { "-∞" }.to_owned();
    }
    let digits = format!("{:.0}", n.abs());
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n.is_sign_negative() {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Thousands-separated rounded integer: `1,234,567`.
pub fn fmt(n: f64) -> String {
    grouped(js_round(n))
}

/// Compact: `1.2K`, `1M`; whole thousands and millions drop the decimal.
pub fn fmt_k(n: f64) -> String {
    let scaled = |unit: f64, suffix: &str| {
        let digits = if n % unit == 0.0 { 0 } else { 1 };
        format!("{}{suffix}", to_fixed(n / unit, digits))
    };
    if n >= MILLION {
        scaled(MILLION, "M")
    } else if n >= THOUSAND {
        scaled(THOUSAND, "K")
    } else {
        to_js_string(js_round(n))
    }
}

/// Percent: one decimal under 10%, else a whole number.
pub fn fmt_pct(frac: f64) -> String {
    let pct = frac * 100.0;
    if pct < 10.0 {
        format!("{}%", to_fixed(pct, 1))
    } else {
        format!("{}%", to_js_string(js_round(pct)))
    }
}

/// Byte size in decimal units: `1.2 MB`, `3.4 KB`, `512 B`.
pub fn fmt_bytes(n: f64) -> String {
    if n >= MILLION {
        format!("{} MB", to_fixed(n / MILLION, 1))
    } else if n >= THOUSAND {
        format!("{} KB", to_fixed(n / THOUSAND, 1))
    } else {
        format!("{} B", to_js_string(n))
    }
}

/// First line of `text`, cut at `max` UTF-16 units plus `…`.
pub fn first_line(text: &str, max: usize) -> String {
    let line = text.split('\n').next().unwrap_or_default();
    super::utf16::clip(line, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_js() {
        assert_eq!(fmt(1_234_567.4), "1,234,567");
        assert_eq!(fmt(999.5), "1,000");
        assert_eq!(fmt_k(1_000_000.0), "1M");
        assert_eq!(fmt_k(1_250_000.0), "1.3M");
        assert_eq!(fmt_k(1500.0), "1.5K");
        assert_eq!(fmt_k(2000.0), "2K");
        assert_eq!(fmt_k(999_999.5), "1000.0K");
        assert_eq!(fmt_k(12.5), "13");
        assert_eq!(fmt_pct(0.0125), "1.3%");
        assert_eq!(fmt_pct(0.469_438), "47%");
        assert_eq!(fmt_bytes(200_000.0), "200.0 KB");
        assert_eq!(fmt_bytes(512.0), "512 B");
    }

    #[test]
    fn zones_keep_a_warn_band() {
        assert_eq!(zone_of(0.3, 0.4), Zone::Ok);
        assert_eq!(zone_of(0.5, 0.4), Zone::Warn);
        assert_eq!(zone_of(0.8, 0.4), Zone::Bad);
        assert_eq!(zone_of(0.85, 0.8), Zone::Warn);
        assert_eq!(zone_of(0.9, 0.8), Zone::Bad);
    }
}
