//! The system time zone for `Date.parse` local time, loaded on first use.
//! The CLI, the server and the desktop app share it, so `TZ` reads the same.

use std::sync::OnceLock;

use super::time::{LocalZone, zone_named};
use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};

/// tzdb names with a POSIX rule shape that Bun reads as UTC.
const POSIX_SHAPED: [&str; 5] = ["EST5EDT", "CST6CDT", "MST7MDT", "PST8PDT", "posixrules"];

/// True when `name` looks like an IANA id: `/`-separated parts of letters,
/// digits, `_`, `+` and `-`. Paths, `..` and POSIX rules fail.
fn iana_shaped(name: &str) -> bool {
    name.split('/').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
    }) && !POSIX_SHAPED.iter().any(|p| p.eq_ignore_ascii_case(name))
}

/// The `TZ` environment variable as the process sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TzVar<'a> {
    Unset,
    Text(&'a str),
    NotUtf8,
}

/// The zone Bun uses for `TZ`. Unset means the OS zone. An IANA name, after
/// an optional `:` and `posix/` or `right/`, is that zone. Anything else
/// (POSIX rules, file paths, unknown names, non-UTF-8) is UTC.
fn zone_for(
    tz: TzVar<'_>,
    lookup: impl Fn(&str) -> Option<TimeZone>,
    system: impl FnOnce() -> TimeZone,
) -> TimeZone {
    let text = match tz {
        TzVar::Unset => return system(),
        TzVar::NotUtf8 => return TimeZone::UTC,
        TzVar::Text(t) => t,
    };
    let name = text.strip_prefix(':').unwrap_or(text);
    let name = ["posix/", "right/"]
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .unwrap_or(name);
    Some(name)
        .filter(|n| iana_shaped(n))
        .and_then(lookup)
        .unwrap_or(TimeZone::UTC)
}

fn load() -> TimeZone {
    let tz = std::env::var_os("TZ");
    let var = match tz.as_deref().map(std::ffi::OsStr::to_str) {
        None => TzVar::Unset,
        Some(Some(t)) => TzVar::Text(t),
        Some(None) => TzVar::NotUtf8,
    };
    zone_for(var, zone_named, TimeZone::system)
}

/// The `TZ` zone as Bun reads it, else the OS zone, else UTC.
#[derive(Debug, Default)]
pub struct SystemZone(OnceLock<TimeZone>);

impl LocalZone for SystemZone {
    fn offset_at_wall_ms(&self, wall_ms: i64) -> i64 {
        let tz = self.0.get_or_init(load);
        let min = Timestamp::MIN.as_millisecond();
        let max = Timestamp::MAX.as_millisecond();
        let Ok(wall) = Timestamp::from_millisecond(wall_ms.clamp(min, max)) else {
            return 0;
        };
        tz.to_ambiguous_timestamp(Offset::UTC.to_datetime(wall))
            .compatible()
            .map_or(0, |at| wall.as_millisecond() - at.as_millisecond())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str) -> SystemZone {
        let tz = TimeZone::get(name).unwrap_or(TimeZone::UTC);
        SystemZone(OnceLock::from(tz))
    }

    /// Expected offsets match Bun `Date.parse` under the same `TZ`.
    #[test]
    fn offsets_follow_the_zone() {
        let ny = zone("America/New_York");
        let wall = |iso: &str| {
            iso.parse::<Timestamp>()
                .map(Timestamp::as_millisecond)
                .unwrap_or_default()
        };
        let hour = 3_600_000;
        assert_eq!(
            ny.offset_at_wall_ms(wall("2026-01-06T10:00:00Z")),
            -5 * hour
        );
        assert_eq!(
            ny.offset_at_wall_ms(wall("2026-03-08T02:30:00Z")),
            -5 * hour
        );
        assert_eq!(
            ny.offset_at_wall_ms(wall("2026-11-01T01:30:00Z")),
            -4 * hour
        );
        assert_eq!(zone("Asia/Tokyo").offset_at_wall_ms(0), 9 * hour);
    }

    /// Hours east of UTC on 2026-07-01 for `TZ`, with UTC as the OS zone.
    fn hours(tz: TzVar<'_>) -> i64 {
        let tz = zone_for(tz, zone_named, || TimeZone::UTC);
        let at = "2026-07-01T00:00:00Z"
            .parse::<Timestamp>()
            .unwrap_or_default();
        i64::from(tz.to_offset(at).seconds()) / 3600
    }

    /// Expected values come from Bun 1.4 under the same `TZ`.
    #[test]
    fn tz_is_read_like_bun() {
        let system = || TimeZone::get("Asia/Tokyo").unwrap_or(TimeZone::UTC);
        let at = "2026-07-01T00:00:00Z"
            .parse::<Timestamp>()
            .unwrap_or_default();
        let unset = zone_for(TzVar::Unset, |_| None, system);
        assert_eq!(unset.to_offset(at).seconds(), 9 * 3600);
        let cases = [
            ("America/New_York", -4),
            (":America/New_York", -4),
            ("asia/tokyo", 9),
            ("posix/Asia/Tokyo", 9),
            ("Etc/GMT-3", 3),
            ("", 0),
            ("UTC", 0),
            ("Foo/Bar", 0),
            ("EST5EDT", 0),
            ("PST8PDT", 0),
            ("UTC+5", 0),
            ("GMT-3", 0),
            ("posixrules", 0),
            ("/usr/share/zoneinfo/Asia/Tokyo", 0),
            ("/etc/localtime", 0),
            (":/etc/localtime", 0),
            ("::Asia/Tokyo", 0),
            ("Asia/Tokyo/", 0),
            (" Asia/Tokyo", 0),
        ];
        let got = cases.map(|(tz, _)| (tz, hours(TzVar::Text(tz))));
        assert_eq!(got, cases);
        assert_eq!(hours(TzVar::NotUtf8), 0);
    }

    /// Legacy aliases resolve even where the OS ships them separately.
    #[test]
    fn legacy_tz_aliases_resolve() {
        assert_eq!(hours(TzVar::Text("Japan")), 9);
        assert_eq!(hours(TzVar::Text("EST")), -5);
    }
}
