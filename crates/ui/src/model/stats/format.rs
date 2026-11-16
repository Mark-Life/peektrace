//! How the stats screen spells numbers, as the inspector's
//! `lib/stats-format.ts` does in a browser.

pub use peektrace_js::inspector::{hms, pct};
pub use peektrace_js::num::{rate_text, to_locale_string as n};
use peektrace_js::time::to_iso_string;

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
    fn shares_counts_and_dates() {
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
