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

/// Percentage from a 0..1 fraction: `12.5%`, and `<0.1%` for a share too
/// small to show.
pub use crate::js::inspector::pct as percent;

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

/// How a waiting call parks, in plain words: `polling loop`.
pub const fn shape_label(shape: super::schema::WaitShape) -> &'static str {
    use super::schema::WaitShape as S;
    use peektrace_protocol::wire::stats::WaitShape as W;
    peektrace_protocol::stats_text::wait_shape_label(match shape {
        S::LoopPoll => W::LoopPoll,
        S::Watcher => W::Watcher,
        S::CurlRetry => W::CurlRetry,
        S::BareDelay => W::BareDelay,
    })
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
    }
}
