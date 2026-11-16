//! The findings band, the two ranked tables and the note strip. Rows come
//! ranked from core and are never re-sorted.

use std::fmt::Write as _;

use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::findings::Finding;
use peektrace_core::stats::format::hms;
use peektrace_core::stats::report::StatsReport;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::layout::{StatsCols, StatsLayout, cell, num, pair_cell, plural, share_cell};
use super::text::Pane;
use crate::draw::{Seg, fill_row, put, seg};
use crate::fmt::{clip, fmt, sanitize, width};
use crate::list::window_slice;
use crate::theme::{self, fg};

/// Writes rows top-down inside `area`, dropping what falls below it.
pub struct Rows<'a> {
    pub buf: &'a mut Buffer,
    pub area: Rect,
    pub y: u16,
}

impl Rows<'_> {
    pub fn line(&mut self, segs: &[Seg]) {
        self.line_sel(segs, false);
    }

    pub fn line_sel(&mut self, segs: &[Seg], selected: bool) {
        if self.y < self.area.bottom() {
            if selected {
                fill_row(
                    self.buf,
                    self.area.x,
                    self.y,
                    self.area.width,
                    Style::new().bg(theme::PANEL_SEL),
                );
            }
            put(self.buf, self.area.x, self.y, self.area.width, segs);
        }
        self.y += 1;
    }
}

fn pane_title(r: &mut Rows<'_>, focused: bool, title: &str, note: &str, w: usize) {
    r.line(&[
        seg(
            "▌",
            fg(if focused {
                theme::PRIMARY
            } else {
                theme::BORDER
            }),
        ),
        seg(
            title,
            fg(if focused {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            }),
        ),
        seg(
            format!("  {}", clip(note, w.saturating_sub(width(title) + 3))),
            fg(theme::TEXT_FAINT),
        ),
    ]);
}

fn more_rows(r: &mut Rows<'_>, count: usize, (start, end): (usize, usize)) {
    let mut parts = Vec::new();
    if start > 0 {
        #[expect(clippy::cast_precision_loss, reason = "row counts")]
        parts.push(format!("↑ {} above", fmt(start as f64)));
    }
    if count > end {
        parts.push(format!("↓ {}", plural(count - end, "more row")));
    }
    let text = if parts.is_empty() {
        " ".to_owned()
    } else {
        format!("  {}", parts.join(" · "))
    };
    r.line(&[seg(text, fg(theme::TEXT_FAINT))]);
}

/// The lead band; collapsed it is its title line alone.
pub fn findings_band(
    r: &mut Rows<'_>,
    findings: &[Finding],
    focused: bool,
    index: usize,
    shown: usize,
    cols: &StatsCols,
) {
    let content = cols.content;
    if shown == 0 {
        let note = format!(
            "{} name a change · f to open",
            plural(findings.len(), "row")
        );
        pane_title(r, focused, "FIX FIRST", &note, content);
        return;
    }
    pane_title(
        r,
        focused,
        "FIX FIRST",
        "rows that name a change  ·  ▲ = a named rule fired",
        content,
    );
    let (start, end) = window_slice(index, findings.len(), shown);
    let title_w = if cols.wide { 29 } else { 24 };
    let gap = if cols.wide { "  " } else { " " };
    for (i, f) in findings.iter().enumerate().take(end).skip(start) {
        let selected = i == index;
        let tight = width(&f.impact) >= 12;
        let title_w = title_w - usize::from(tight);
        let impact = if tight {
            format!(" {}", f.impact)
        } else {
            num(&f.impact, 12)
        };
        let spread_w = content.saturating_sub(2 + title_w + width(&impact) + gap.len());
        let marker = if f.detector.is_some() {
            seg("▲ ", fg(theme::WARN))
        } else {
            seg("· ", fg(theme::TEXT_FAINT))
        };
        r.line_sel(
            &[
                marker,
                seg(
                    cell(&f.title, title_w),
                    fg(if selected {
                        theme::PRIMARY
                    } else {
                        theme::TEXT
                    }),
                ),
                seg(impact, fg(theme::TEXT)),
                seg(
                    format!("{gap}{}", cell(&f.spread, spread_w)),
                    fg(theme::TEXT_DIM),
                ),
            ],
            selected,
        );
        r.line_sel(
            &[seg(
                format!("  {}", clip(&sanitize(&f.note), content.saturating_sub(2))),
                fg(theme::TEXT_FAINT),
            )],
            selected,
        );
    }
    let rest = findings.len() - end;
    let tail = if rest > 0 {
        format!("  {} · f", plural(rest, "more finding"))
    } else {
        " ".to_owned()
    };
    r.line(&[seg(tail, fg(theme::TEXT_FAINT))]);
}

fn fail_row(row: &FailRow, selected: bool, c: &StatsCols) -> Vec<Seg> {
    let f = &c.fail;
    let g = f.gap;
    let mut segs = vec![
        seg(if selected { "▸" } else { " " }, fg(theme::PRIMARY)),
        seg(
            if row.detector.is_some() { "▲" } else { " " },
            fg(theme::WARN),
        ),
        seg(num(&fmt(row.sessions), f.sessions), fg(theme::TEXT)),
        seg(
            format!("{g}{}", num(&fmt(row.rows), f.calls)),
            fg(theme::TEXT_DIM),
        ),
        seg(
            format!("{g}{}", num(&fmt(row.flagged), f.flagged)),
            fg(theme::TEXT_DIM),
        ),
        seg(
            format!("{g}{}", num(&fmt(row.projects), f.proj)),
            fg(theme::TEXT_FAINT),
        ),
    ];
    if f.r#where > 0 {
        segs.push(seg(
            format!("{g}{}", cell(&row.r#where, f.r#where)),
            fg(theme::TEXT_FAINT),
        ));
    }
    segs.push(seg(
        format!("{g}{}", cell(&row.label, f.what)),
        fg(if selected {
            theme::PRIMARY
        } else {
            theme::TEXT
        }),
    ));
    segs
}

pub fn fail_table(
    r: &mut Rows<'_>,
    rows: &[FailRow],
    focused: bool,
    index: usize,
    visible: usize,
    c: &StatsCols,
) {
    let note = format!(
        "ranked by sessions spanned  ·  {} kept",
        plural(rows.len(), "row")
    );
    pane_title(r, focused, "WHAT FAILS", &note, c.content);
    let f = &c.fail;
    let g = f.gap;
    let mut rest = format!(
        "{g}{}{g}{}{g}{}{g}",
        num("CALLS", f.calls),
        num("FLAGGED", f.flagged),
        num("PROJ", f.proj)
    );
    if f.r#where > 0 {
        let _ = write!(rest, "{}{g}", crate::fmt::pad_end("WHERE", f.r#where));
    }
    rest.push_str("WHAT");
    r.line(&[
        seg(
            format!("  {}", num("SESSIONS▼", f.sessions)),
            fg(theme::TEXT_DIM),
        ),
        seg(rest, fg(theme::TEXT_FAINT)),
    ]);
    if rows.is_empty() {
        r.line(&[seg(
            "  No failing calls in this window.",
            fg(theme::TEXT_FAINT),
        )]);
    }
    let win = window_slice(index, rows.len(), visible);
    for (i, row) in rows.iter().enumerate().take(win.1).skip(win.0) {
        r.line_sel(&fail_row(row, i == index, c), i == index);
    }
    more_rows(r, rows.len(), win);
}

fn time_row(row: &TimeRow, bash_sec: f64, selected: bool, c: &StatsCols) -> Vec<Seg> {
    let t = &c.time;
    let g = t.gap;
    let capped = pair_cell(row.capped_runs, row.capped_sec, t.capped);
    let bg = pair_cell(row.bg_runs, row.bg_sec, t.bg);
    let warn_or = |v: &str| {
        fg(if v == "—" {
            theme::TEXT_FAINT
        } else {
            theme::WARN
        })
    };
    let mut segs = vec![
        seg(if selected { "▸ " } else { "  " }, fg(theme::PRIMARY)),
        seg(
            cell(row.label, t.job),
            fg(if selected {
                theme::PRIMARY
            } else {
                theme::TEXT
            }),
        ),
        seg(
            format!("{g}{}", num(&hms(row.total_sec), t.time)),
            fg(theme::TEXT),
        ),
        seg(
            format!("{g}{}", num(&share_cell(row.share, bash_sec), t.share)),
            fg(theme::TEXT_DIM),
        ),
        seg(
            format!(
                "{g}{}{g}{}",
                num(&fmt(row.runs), t.runs),
                num(&fmt(row.co_present_runs), t.co_present)
            ),
            fg(theme::TEXT_FAINT),
        ),
    ];
    if t.median > 0 {
        segs.push(seg(
            format!("{g}{}", num(&hms(row.median_sec), t.median)),
            fg(theme::TEXT_DIM),
        ));
    }
    if t.p95 > 0 {
        segs.push(seg(
            format!("{g}{}", num(&hms(row.p95_sec), t.p95)),
            fg(theme::TEXT_DIM),
        ));
    }
    segs.push(seg(
        format!("{g}{}", num(&capped, t.capped)),
        warn_or(&capped),
    ));
    segs.push(seg(format!("{g}{}", num(&bg, t.bg)), warn_or(&bg)));
    segs
}

pub fn time_table(
    r: &mut Rows<'_>,
    report: &StatsReport,
    focused: bool,
    index: usize,
    visible: usize,
    c: &StatsCols,
) {
    let rows = &report.time;
    let note = if c.wide {
        "ranked by seconds charged  ·  every line charged to exactly one job".to_owned()
    } else {
        format!(
            "by seconds charged · one line, one job · {}",
            plural(rows.len(), "row")
        )
    };
    pane_title(r, focused, "WHAT COSTS TIME", &note, c.content);
    let t = &c.time;
    let g = t.gap;
    let mut rest = format!(
        "{g}{}{g}{}{g}{}",
        num("SHARE", t.share),
        num("RUNS", t.runs),
        num("CO-PRESENT", t.co_present)
    );
    if t.median > 0 {
        let _ = write!(rest, "{g}{}", num("MEDIAN", t.median));
    }
    if t.p95 > 0 {
        let _ = write!(rest, "{g}{}", num("P95", t.p95));
    }
    let _ = write!(rest, "{g}{}{g}{}", num("CAPPED", t.capped), num("BG", t.bg));
    r.line(&[
        seg(
            format!("  {}", crate::fmt::pad_end("JOB", t.job)),
            fg(theme::TEXT_FAINT),
        ),
        seg(format!("{g}{}", num("TIME▼", t.time)), fg(theme::TEXT_DIM)),
        seg(rest, fg(theme::TEXT_FAINT)),
    ]);
    if rows.is_empty() {
        r.line(&[seg(
            "  No shell seconds in this window.",
            fg(theme::TEXT_FAINT),
        )]);
    }
    let win = window_slice(index, rows.len(), visible);
    for (i, row) in rows.iter().enumerate().take(win.1).skip(win.0) {
        r.line_sel(
            &time_row(row, report.corpus.bash_sec, i == index, c),
            i == index,
        );
    }
    more_rows(r, rows.len(), win);
}

/// The body: band, both tables, then the note strip at the bottom.
#[expect(clippy::too_many_arguments, reason = "one draw call")]
pub fn body(
    buf: &mut Buffer,
    area: Rect,
    report: &StatsReport,
    findings: &[Finding],
    indexes: [usize; 3],
    pane: Pane,
    layout: StatsLayout,
    shown: usize,
    notes: &[String],
    cols: &StatsCols,
) {
    let mut r = Rows {
        buf,
        area,
        y: area.y,
    };
    findings_band(
        &mut r,
        findings,
        pane == Pane::Findings,
        indexes[0],
        shown,
        cols,
    );
    r.line(&[]);
    fail_table(
        &mut r,
        &report.fails,
        pane == Pane::Fails,
        indexes[1],
        layout.fail_rows,
        cols,
    );
    r.line(&[]);
    time_table(
        &mut r,
        report,
        pane == Pane::Time,
        indexes[2],
        layout.time_rows,
        cols,
    );
    if notes.is_empty() {
        return;
    }
    let need = u16::try_from(notes.len() + 1).unwrap_or(u16::MAX);
    r.y = r.y.max(area.bottom().saturating_sub(need)) + 1;
    for (i, line) in notes.iter().enumerate() {
        let (label, style, text_style) = if i == 0 {
            ("NOTE  ", fg(theme::ACCENT), fg(theme::TEXT))
        } else {
            ("      ", fg(theme::TEXT_FAINT), fg(theme::TEXT_FAINT))
        };
        r.line(&[
            seg(label, style),
            seg(
                clip(&sanitize(line), cols.content.saturating_sub(6)),
                text_style,
            ),
        ]);
    }
}
