//! The system time zone for `Date.parse` local time, loaded on first use.

use std::sync::OnceLock;

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};
use peektrace_core::js::time::LocalZone;

/// `TZ`, else the OS zone, else UTC.
#[derive(Debug, Default)]
pub struct SystemZone(OnceLock<TimeZone>);

impl LocalZone for SystemZone {
    fn offset_at_wall_ms(&self, wall_ms: i64) -> i64 {
        let tz = self.0.get_or_init(TimeZone::system);
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
}
