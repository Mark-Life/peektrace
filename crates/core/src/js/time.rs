//! JS dates: `Date.parse` and `toISOString` from `peektrace_js::time`, plus
//! file times as `fs.stat` reports them and IANA zones by name.

use std::time::{SystemTime, UNIX_EPOCH};

pub use peektrace_js::time::*;

/// `stat().mtime.getTime()`: Node builds `mtimeMs` as the f64
/// `sec * 1e3 + nsec / 1e6` and `new Date` truncates it, so a sub-ms part
/// close to the next ms rounds up (e.g. ns `…122999906` reads as `…123`).
pub fn file_time_ms(t: SystemTime) -> i64 {
    let (sec, nsec) = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => (
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            d.subsec_nanos(),
        ),
        Err(e) => {
            let d = e.duration();
            let sec = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
            match d.subsec_nanos() {
                0 => (-sec, 0),
                n => (-sec - 1, 1_000_000_000 - n),
            }
        }
    };
    timespec_ms(sec, nsec)
}

/// `new Date(sec * 1e3 + nsec / 1e6).getTime()` for a POSIX timespec.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "mirrors the f64 math of Node's mtimeMs"
)]
fn timespec_ms(sec: i64, nsec: u32) -> i64 {
    (sec as f64 * 1e3 + f64::from(nsec) / 1e6).trunc() as i64
}

/// An IANA zone by name or alias, as ICU knows it. The OS database comes
/// first; names it lacks (Debian and Ubuntu ship legacy aliases such as
/// `Asia/Calcutta` in a separate package) fall back to the bundled copy.
pub fn zone_named(name: &str) -> Option<jiff::tz::TimeZone> {
    jiff::tz::TimeZone::get(name)
        .or_else(|_| jiff::tz::TimeZoneDatabase::bundled().get(name))
        .ok()
}

#[cfg(test)]
mod zone_tests {
    use super::zone_named;

    #[test]
    fn legacy_aliases_resolve_without_the_os_package() {
        for name in ["Asia/Calcutta", "Europe/Kiev", "US/Pacific", "Japan", "EST"] {
            assert!(zone_named(name).is_some(), "{name}");
        }
        assert!(zone_named("Mars/Base").is_none());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn file_time_rounds_like_node_mtime_ms() {
        let at = |ns: u64| file_time_ms(UNIX_EPOCH + Duration::from_nanos(ns));
        // `fs.utimesSync(p, a, 1786698711.123)` stores this ns value.
        assert_eq!(at(1_786_698_711_122_999_906), 1_786_698_711_123);
        assert_eq!(at(1_786_698_711_851_000_000), 1_786_698_711_851);
        assert_eq!(at(1_786_698_711_122_500_000), 1_786_698_711_122);
        assert_eq!(
            file_time_ms(UNIX_EPOCH - Duration::from_millis(1500)),
            -1500
        );
        assert_eq!(file_time_ms(UNIX_EPOCH - Duration::from_nanos(1)), 0);
    }
}
