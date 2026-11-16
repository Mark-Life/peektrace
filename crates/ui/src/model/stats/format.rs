//! How the stats screen spells numbers, as the inspector's
//! `lib/stats-format.ts` does in a browser.

use peektrace_js::num::{js_round, to_fixed, to_js_string};
pub use peektrace_js::num::{rate_text, to_locale_string as n};
use peektrace_js::time::to_iso_string;

const SEC_PER_MIN: f64 = 60.0;
const SEC_PER_HOUR: f64 = 3600.0;

fn pad2(s: String) -> String {
    if s.len() < 2 { format!("0{s}") } else { s }
}

/// `4h12m`, `3m07s`, `0.42s`. A minute rounds its seconds, so `119.6` is
/// `1m60s`, as the inspector prints it.
pub fn hms(sec: f64) -> String {
    if sec < SEC_PER_MIN {
        let digits = if sec < 10.0 { 2 } else { 0 };
        return format!("{}s", to_fixed(sec, digits));
    }
    if sec < SEC_PER_HOUR {
        let m = (sec / SEC_PER_MIN).floor();
        let s = js_round(sec - m * SEC_PER_MIN);
        return format!("{}m{}s", to_js_string(m), pad2(to_js_string(s)));
    }
    let h = (sec / SEC_PER_HOUR).floor();
    let m = ((sec - h * SEC_PER_HOUR) / SEC_PER_MIN).floor();
    format!("{}h{}m", to_js_string(h), pad2(to_js_string(m)))
}

/// A 0-1 share as a percentage with two decimals.
pub fn pct(share: f64) -> String {
    format!("{}%", to_fixed(share * 100.0, 2))
}

/// `new Date(ms).toLocaleString()` in en-US, `offset_ms` east of UTC:
/// `11/14/2026, 9:11:55 AM`.
#[expect(clippy::cast_possible_truncation, reason = "epoch ms")]
pub fn at(epoch_ms: f64, offset_ms: f64) -> String {
    if !epoch_ms.is_finite() {
        return "Invalid Date".to_owned();
    }
    let iso = to_iso_string((epoch_ms + offset_ms).floor() as i64);
    let Some((date, time)) = iso.split_once('T') else {
        return iso;
    };
    let mut d = date.rsplitn(3, '-');
    let (day, month, year) = (
        d.next().unwrap_or_default(),
        d.next().unwrap_or_default(),
        d.next().unwrap_or_default(),
    );
    let num = |s: &str| s.trim_start_matches('0').parse::<u32>().unwrap_or(0);
    let hour = num(&time[..2.min(time.len())]);
    let rest = time.get(2..8).unwrap_or_default();
    let (h12, half) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    let year = year.trim_start_matches('+');
    let year = year
        .strip_prefix('0')
        .map_or(year, |y| y.trim_start_matches('0'));
    format!("{}/{}/{year}, {h12}{rest} {half}", num(month), num(day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hms_follows_the_inspector() {
        let cases = [
            (0.0, "0.00s"),
            (0.4, "0.40s"),
            (9.995, "9.99s"),
            (10.0, "10s"),
            (42.0, "42s"),
            (59.4, "59s"),
            (59.6, "60s"),
            (60.0, "1m00s"),
            (119.6, "1m60s"),
            (187.0, "3m07s"),
            (3599.6, "59m60s"),
            (3600.0, "1h00m"),
            (18277.0, "5h04m"),
            (-1.0, "-1.00s"),
        ];
        for (sec, want) in cases {
            assert_eq!(hms(sec), want, "{sec}");
        }
    }

    #[test]
    fn shares_counts_and_dates() {
        assert_eq!(pct(0.0), "0.00%");
        assert_eq!(pct(0.16), "16.00%");
        assert_eq!(pct(0.157_1), "15.71%");
        assert_eq!(pct(1.0), "100.00%");
        assert_eq!(n(0.0), "0");
        assert_eq!(n(1234.0), "1,234");
        assert_eq!(n(1_234.567_8), "1,234.568");
        assert_eq!(n(-0.4), "-0.4");
        assert_eq!(n(1e21), "1,000,000,000,000,000,000,000");
        assert_eq!(at(1_794_647_515_696.0, 0.0), "11/14/2026, 9:11:55 AM");
        assert_eq!(at(1_794_614_400_000.0, 0.0), "11/14/2026, 12:00:00 AM");
        assert_eq!(at(1_794_657_600_000.0, 0.0), "11/14/2026, 12:00:00 PM");
        assert_eq!(
            at(1_794_614_400_000.0, -3_600_000.0),
            "11/13/2026, 11:00:00 PM"
        );
        assert_eq!(rate_text(0.125, 1.0), "0.13");
        assert_eq!(rate_text(0.001, 1.0), "<0.01");
        assert_eq!(rate_text(12.5, 1.0), "13");
    }
}
