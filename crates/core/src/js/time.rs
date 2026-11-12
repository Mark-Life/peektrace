//! `Date.parse` for ISO strings and `Date.prototype.toISOString`, following
//! `JavaScriptCore` (Bun) so timestamps sort and filter the same as in TS.

const MS_PER_DAY: i64 = 86_400_000;
const MAX_TIME: i64 = 8_640_000_000_000_000;

/// Days since 1970-01-01 for a proleptic Gregorian date. `day` may overflow
/// the month (`02-30` is March 2), as `Date.parse` allows.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 + (day - 1)
}

/// `(year, month, day)` for days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Byte cursor over the input.
struct Cursor<'a> {
    s: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn eat(&mut self, b: u8) -> bool {
        let hit = self.peek() == Some(b);
        if hit {
            self.at += 1;
        }
        hit
    }

    /// Exactly `n` ASCII digits as a number.
    fn digits(&mut self, n: usize) -> Option<i64> {
        let end = self.at.checked_add(n)?;
        let slice = self.s.get(self.at..end)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at = end;
        Some(
            slice
                .iter()
                .fold(0, |acc, d| acc * 10 + i64::from(d - b'0')),
        )
    }

    /// One or more digits; the value of the first three as milliseconds.
    fn fraction_ms(&mut self) -> Option<i64> {
        let run = self.s[self.at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if run == 0 {
            return None;
        }
        let ms = (0..3).fold(0, |acc, i| {
            let d = if i < run {
                i64::from(self.s[self.at + i] - b'0')
            } else {
                0
            };
            acc * 10 + d
        });
        self.at += run;
        Some(ms)
    }
}

fn year(c: &mut Cursor<'_>) -> Option<i64> {
    match c.peek()? {
        sign @ (b'+' | b'-') => {
            c.at += 1;
            let y = c.digits(6)?;
            if sign == b'-' {
                (y != 0).then_some(-y)
            } else {
                Some(y)
            }
        }
        _ => c.digits(4),
    }
}

/// Milliseconds into the day for `HH:mm[:ss[.fff]]`.
fn time_of_day(c: &mut Cursor<'_>) -> Option<i64> {
    let hours = c.digits(2)?;
    if !c.eat(b':') {
        return None;
    }
    let minutes = c.digits(2)?;
    let (seconds, ms) = if c.eat(b':') {
        let seconds = c.digits(2)?;
        let ms = if c.eat(b'.') { c.fraction_ms()? } else { 0 };
        (seconds, ms)
    } else {
        (0, 0)
    };
    let valid = minutes < 60
        && seconds < 60
        && (hours < 24 || (hours == 24 && minutes == 0 && seconds == 0 && ms == 0));
    valid.then_some(((hours * 60 + minutes) * 60 + seconds) * 1000 + ms)
}

/// Local time as `Date` sees it: the offset from UTC, in ms, that applies
/// at a wall-clock time (given as ms since 1970 read as if UTC). Skipped and
/// repeated wall times resolve the way `Date.parse` does: a skipped time
/// moves forward, a repeated one takes the earlier instant.
pub trait LocalZone {
    fn offset_at_wall_ms(&self, wall_ms: i64) -> i64;
}

/// UTC as the local zone.
#[derive(Debug, Clone, Copy, Default)]
pub struct Utc;

impl LocalZone for Utc {
    fn offset_at_wall_ms(&self, _wall_ms: i64) -> i64 {
        0
    }
}

/// The zone part of an ISO string.
enum Zone {
    /// Minutes east of UTC.
    Offset(i64),
    /// No zone given.
    Absent,
}

/// `Z`, `±HH[:]mm` or nothing; `None` means a malformed zone.
fn zone(c: &mut Cursor<'_>) -> Option<Zone> {
    match c.peek() {
        Some(b'Z' | b'z') => {
            c.at += 1;
            Some(Zone::Offset(0))
        }
        Some(sign @ (b'+' | b'-')) => {
            c.at += 1;
            let hours = c.digits(2)?;
            c.eat(b':');
            let minutes = c.digits(2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = hours * 60 + minutes;
            Some(Zone::Offset(if sign == b'-' { -offset } else { offset }))
        }
        _ => Some(Zone::Absent),
    }
}

/// `Date.parse` for the ISO format `JavaScriptCore` accepts, in epoch ms.
/// `None` stands for NaN. A date-time with no offset is `local` time; a date
/// alone is UTC. Non-ISO strings, which JS hands to a lenient legacy parser,
/// are `None`.
pub fn parse_date(s: &str, local: &dyn LocalZone) -> Option<i64> {
    let mut c = Cursor {
        s: s.as_bytes(),
        at: 0,
    };
    let year = year(&mut c)?;
    let (month, day) = if c.eat(b'-') {
        let month = c.digits(2)?;
        let day = if c.eat(b'-') { c.digits(2)? } else { 1 };
        (month, day)
    } else {
        (1, 1)
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let has_time = matches!(c.peek(), Some(b'T' | b't'));
    let time = if has_time {
        c.at += 1;
        time_of_day(&mut c)?
    } else {
        0
    };
    let offset = zone(&mut c)?;
    if c.at != c.s.len() {
        return None;
    }
    let wall = days_from_civil(year, month, day) * MS_PER_DAY + time;
    let offset_ms = match offset {
        Zone::Offset(minutes) => minutes * 60_000,
        Zone::Absent if has_time => local.offset_at_wall_ms(wall),
        Zone::Absent => 0,
    };
    let ms = wall - offset_ms;
    (ms.abs() <= MAX_TIME).then_some(ms)
}

/// `Date.UTC(year, month0, day)`: `month0` is 0-based and may overflow the
/// year, `day` may overflow or underflow the month.
pub fn utc_ms(year: i64, month0: i64, day: i64) -> i64 {
    let y = year + month0.div_euclid(12);
    let m = month0.rem_euclid(12) + 1;
    (days_from_civil(y, m, 1) + day - 1) * MS_PER_DAY
}

/// `new Date(ms).toISOString()`.
pub fn to_iso_string(ms: i64) -> String {
    let days = ms.div_euclid(MS_PER_DAY);
    let rem = ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        let sign = if year < 0 { '-' } else { '+' };
        format!("{sign}{:06}", year.abs())
    };
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values measured with Bun 1.4.4 `Date.parse` under `TZ=UTC`.
    const CASES: &[(&str, Option<i64>)] = &[
        ("2026-01-31", Some(1_769_817_600_000)),
        ("2026-01-31T10:00Z", Some(1_769_853_600_000)),
        ("2026-01-31T10Z", None),
        ("2026-01-31T10:00:00Z", Some(1_769_853_600_000)),
        ("2026-01-31T10:00:00.1Z", Some(1_769_853_600_100)),
        ("2026-01-31T10:00:00.12Z", Some(1_769_853_600_120)),
        ("2026-01-31T10:00:00.1234Z", Some(1_769_853_600_123)),
        ("2026-01-31T10:00:00.123456789Z", Some(1_769_853_600_123)),
        ("2026-01-31T10:00+0100", Some(1_769_850_000_000)),
        ("2026-01-31T10:00+01:00", Some(1_769_850_000_000)),
        ("2026-01-31T10:00-0530", Some(1_769_873_400_000)),
        ("2026-01-31T24:00Z", Some(1_769_904_000_000)),
        ("2026-01-31T24:00:00Z", Some(1_769_904_000_000)),
        ("2026-01-31T24:00:01Z", None),
        ("2026-01-31T24:00:00.001Z", None),
        ("2026-01-31T23:59:60Z", None),
        ("2026-01-31T25:00Z", None),
        ("2026-01-31T10:60Z", None),
        ("2026-01-31T1:2Z", None),
        ("2026-01-31T10:00:00:00Z", None),
        ("2026-02-30T00:00:00Z", Some(1_772_409_600_000)),
        ("2026-02-29", Some(1_772_323_200_000)),
        ("2024-02-29", Some(1_709_164_800_000)),
        ("2026-02-31", Some(1_772_496_000_000)),
        ("2026-04-31", Some(1_777_593_600_000)),
        ("2026-13-01", None),
        ("2026-00-01", None),
        ("2026-01-00", None),
        ("2026-01-32", None),
        ("0000-01-01", Some(-62_167_219_200_000)),
        ("+002026-01-31T00:00:00Z", Some(1_769_817_600_000)),
        ("-000001-01-01T00:00:00Z", Some(-62_198_755_200_000)),
        ("-000000-01-01T00:00:00Z", None),
        ("2026-01-31T10:00:00.Z", None),
        ("2026-01-31T10:00.", None),
        ("2026-01-31T10:00.5Z", None),
        ("2026-01-31t10:00z", Some(1_769_853_600_000)),
        ("2026-01-31T10:00:00.000z", Some(1_769_853_600_000)),
        ("2026-01-31T10:00:00", Some(1_769_853_600_000)),
        ("2026-01-31T10:00:00.000+01", None),
        ("2026", Some(1_767_225_600_000)),
        ("2026-01", Some(1_767_225_600_000)),
        ("2026-11-11T16:39:31.257Z", Some(1_794_415_171_257)),
        ("garbage", None),
        ("", None),
        ("2026-01-31T10:00:00+01:00x", None),
        ("2026-01-31T10:00:00+0", None),
        ("2026-01-31T10:00:00+1:00", None),
        ("2026-01-31T10:00:00+24:00", None),
        ("2026-01-31T10:00:00+23:59", Some(1_769_767_260_000)),
        ("2026-01-31T10:00:00+01:60", None),
        ("2026-01-31T10:00:00+0100Z", None),
        ("2026-01-31T10:00:00 Z", None),
        ("275760-09-13T00:00:00Z", None),
        ("+275760-09-13T00:00:00Z", Some(8_640_000_000_000_000)),
        ("+275760-09-13T00:00:00.001Z", None),
        ("-271821-04-20T00:00:00Z", Some(-8_640_000_000_000_000)),
        ("-271821-04-19T23:59:59.999Z", None),
        ("2026-01-31T10:00:00,123Z", None),
        ("2026-01-31T10:00:00.1234+01:00", Some(1_769_850_000_123)),
        ("2026-01-31Z", Some(1_769_817_600_000)),
        ("2026-01Z", Some(1_767_225_600_000)),
        ("2026-01-31T-1:00Z", None),
        ("2026-01-31T10:00:00+01:00:00", None),
        ("1969-12-31T23:59:59.999Z", Some(-1)),
        ("9999-12-31T23:59:59.999Z", Some(253_402_300_799_999)),
        ("20260131", None),
        ("2026-01-31T10:0Z", None),
        ("2026-01-31T 10:00Z", None),
        ("2026-12-31T23:59:59.9999Z", Some(1_798_761_599_999)),
        ("2026-01-31T10:00:00-00:00", Some(1_769_853_600_000)),
        ("2026-01-31T10:00:00+00", None),
    ];

    #[test]
    fn parses_like_bun() {
        for &(input, want) in CASES {
            assert_eq!(parse_date(input, &Utc), want, "{input:?}");
        }
    }

    /// Fixed `+09:00`, as `TZ=Asia/Tokyo`.
    struct Tokyo;

    impl LocalZone for Tokyo {
        fn offset_at_wall_ms(&self, _wall_ms: i64) -> i64 {
            9 * 3_600_000
        }
    }

    /// Expected values from Bun `Date.parse` under `TZ=Asia/Tokyo`.
    #[test]
    fn date_times_without_offset_are_local() {
        assert_eq!(
            parse_date("2026-01-06T10:00:00", &Tokyo),
            Some(1_767_661_200_000)
        );
        assert_eq!(parse_date("2026-01-06", &Tokyo), Some(1_767_657_600_000));
        assert_eq!(
            parse_date("2026-01-06T05:00:00Z", &Tokyo),
            Some(1_767_675_600_000)
        );
    }

    #[test]
    fn formats_iso() {
        assert_eq!(to_iso_string(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(to_iso_string(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(to_iso_string(1_794_415_171_257), "2026-11-11T16:39:31.257Z");
        assert_eq!(
            to_iso_string(8_640_000_000_000_000),
            "+275760-09-13T00:00:00.000Z"
        );
        assert_eq!(
            to_iso_string(-62_198_755_200_000),
            "-000001-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn round_trips() {
        for ms in [
            0,
            1,
            -1,
            951_782_400_000,
            1_709_164_800_000,
            253_402_300_799_999,
        ] {
            assert_eq!(parse_date(&to_iso_string(ms), &Utc), Some(ms));
        }
    }
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
