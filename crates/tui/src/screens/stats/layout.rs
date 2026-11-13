//! Stats layout from the terminal size: rows per band, column widths, and
//! the cells both tables print.

use peektrace_core::stats::format::{hms, percent};

use crate::fmt::{clip, fmt, pad_end, pad_start, sanitize, width};

/// Below this the screen says it is too small.
pub const MIN_HEIGHT: u16 = 24;
pub const MIN_WIDTH: u16 = 80;

const SHELL_ROWS: i32 = 2;
const FIXED_ROWS: i32 = 21;

/// Rows each band gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsLayout {
    pub fail_rows: usize,
    /// Findings shown, two lines each; 0 collapses the band to its title.
    pub findings: usize,
    pub note_lines: usize,
    pub time_rows: usize,
}

const fn findings_for(avail: i32) -> usize {
    if avail >= 12 {
        3
    } else if avail >= 10 {
        2
    } else if avail >= 8 {
        1
    } else {
        0
    }
}

/// Split `height` between the bands; the note strip sheds lines last.
pub fn layout_of(height: u16) -> StatsLayout {
    let avail = i32::from(height) - SHELL_ROWS - FIXED_ROWS;
    let findings = findings_for(avail);
    let mut rows = if findings == 0 {
        avail + 1
    } else {
        avail - 2 * i32::try_from(findings).unwrap_or(0)
    };
    let mut note_lines = 4;
    if rows < 6 {
        note_lines = 2;
        rows += 2;
    }
    if rows < 6 {
        note_lines = 0;
        rows += 3;
    }
    #[expect(clippy::cast_possible_truncation, reason = "a row count")]
    let time = (peektrace_core::js::num::js_round(f64::from(rows) * 0.45) as i32).clamp(3, 12);
    StatsLayout {
        fail_rows: usize::try_from((rows - time).clamp(3, 20)).unwrap_or(3),
        findings,
        note_lines,
        time_rows: usize::try_from(time).unwrap_or(3),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailCols {
    pub gap: &'static str,
    pub sessions: usize,
    pub calls: usize,
    pub flagged: usize,
    pub proj: usize,
    /// 0 when dropped.
    pub r#where: usize,
    pub what: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeCols {
    pub gap: &'static str,
    pub job: usize,
    pub time: usize,
    pub share: usize,
    pub runs: usize,
    pub co_present: usize,
    /// 0 when dropped, as is `p95`.
    pub median: usize,
    pub p95: usize,
    pub capped: usize,
    pub bg: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsCols {
    pub content: usize,
    pub wide: bool,
    pub fail: FailCols,
    pub time: TimeCols,
}

/// Content width inside the shell padding.
pub const fn content_width(width: u16) -> usize {
    (width as usize).saturating_sub(2)
}

/// Column widths; below 96 the `WHERE`, `MEDIAN` and `P95` columns go.
pub fn columns_of(content: usize, measured_share: bool) -> StatsCols {
    let wide = content >= 96;
    let gap = if wide { "  " } else { " " };
    let share = if measured_share { 6 } else { 12 };
    let slack = share - 6;
    StatsCols {
        content,
        wide,
        fail: FailCols {
            gap,
            sessions: 9,
            calls: 6,
            flagged: 7,
            proj: 4,
            r#where: if wide { 16 } else { 0 },
            what: content.saturating_sub(if wide { 54 } else { 32 }).max(8),
        },
        time: TimeCols {
            gap,
            job: content
                .saturating_sub(if wide { 82 } else { 59 } + slack)
                .max(8),
            time: 7,
            share,
            runs: 7,
            co_present: 10,
            median: if wide { 7 } else { 0 },
            p95: if wide { 6 } else { 0 },
            capped: 11,
            bg: 10,
        },
    }
}

/// `1 row` / `2 rows`.
pub fn plural(count: usize, word: &str) -> String {
    #[expect(clippy::cast_precision_loss, reason = "row counts")]
    let n = fmt(count as f64);
    format!("{n} {word}{}", if count == 1 { "" } else { "s" })
}

/// Left-aligned: clipped to `w`, then padded.
pub fn cell(text: &str, w: usize) -> String {
    pad_end(&clip(&sanitize(text), w), w)
}

/// Right-aligned, never truncated.
pub fn num(text: &str, w: usize) -> String {
    pad_start(text, w)
}

pub fn share_cell(fraction: f64, bash_sec: f64) -> String {
    if bash_sec > 0.0 {
        percent(fraction)
    } else {
        "not measured".to_owned()
    }
}

/// `15/1h41m`, `—` at zero, or the count alone when the pair is too wide.
pub fn pair_cell(runs: f64, sec: f64, w: usize) -> String {
    if runs == 0.0 {
        return "—".to_owned();
    }
    let pair = format!("{}/{}", fmt(runs), hms(sec));
    if width(&pair) <= w { pair } else { fmt(runs) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_and_time_columns_fill_the_width() {
        for content in [98, 78] {
            let c = columns_of(content, true);
            let f = &c.fail;
            let gaps = if f.r#where > 0 { 5 } else { 4 };
            assert_eq!(
                2 + f.sessions
                    + f.calls
                    + f.flagged
                    + f.proj
                    + f.r#where
                    + f.what
                    + gaps * f.gap.len(),
                content
            );
            let t = &c.time;
            let gaps = if t.median > 0 { 8 } else { 6 };
            assert_eq!(
                2 + t.job
                    + t.time
                    + t.share
                    + t.runs
                    + t.co_present
                    + t.median
                    + t.p95
                    + t.capped
                    + t.bg
                    + gaps * t.gap.len(),
                content
            );
        }
    }

    #[test]
    fn unmeasured_share_takes_room_from_the_job_column() {
        let (m, n) = (columns_of(98, true).time, columns_of(98, false).time);
        assert_eq!(share_cell(0.5, 0.0), "not measured");
        assert_eq!(n.share, m.share + "not measured".len() - 6);
        assert_eq!(n.job, m.job - (n.share - m.share));
    }

    #[test]
    fn narrow_layout_drops_wide_columns() {
        let c = columns_of(78, true);
        assert!(!c.wide);
        assert_eq!((c.fail.r#where, c.time.median, c.time.p95), (0, 0, 0));
    }

    #[test]
    fn layout_sheds_the_note_strip_last() {
        assert_eq!(
            layout_of(44),
            StatsLayout {
                fail_rows: 8,
                findings: 3,
                note_lines: 4,
                time_rows: 7
            }
        );
        let short = layout_of(24);
        assert_eq!((short.findings, short.note_lines), (0, 0));
        assert!(short.fail_rows >= 3 && short.time_rows >= 3);
        let full = layout_of(48);
        assert_eq!((full.fail_rows, full.time_rows), (10, 9));
    }

    #[test]
    fn six_figure_pairs_drop_to_the_count() {
        assert_eq!(pair_cell(0.0, 0.0, 11), "—");
        assert_eq!(pair_cell(15.0, 6060.0, 11), "15/1h41m");
        assert_eq!(pair_cell(123_456.0, 400_000.0, 11), "123,456");
    }
}
