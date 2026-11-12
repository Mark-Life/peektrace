//! Copies of `peektrace_core::js::time` (core does not build for wasm32):
//! `Date.parse` for ISO strings, read with no local zone, and day stepping.

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
/// `None` stands for NaN. A date-time with no offset reads as UTC, as with
/// `TZ=UTC`. Non-ISO strings, which JS hands to a lenient legacy parser,
/// are `None`.
pub fn parse_date(s: &str) -> Option<i64> {
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
        Zone::Absent => 0,
    };
    let ms = wall - offset_ms;
    (ms.abs() <= MAX_TIME).then_some(ms)
}

/// `YYYY-MM-DD` of the UTC day `ms` falls on.
pub fn day_key(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(MS_PER_DAY));
    format!("{y:04}-{m:02}-{d:02}")
}

pub const DAY_MS: i64 = MS_PER_DAY;
