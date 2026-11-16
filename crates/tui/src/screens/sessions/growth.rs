//! The growth view: the chart as block-glyph columns with a y gutter, the
//! dumb-zone line, peak and compaction markers, turn ticks and a legend.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::growth_chart::{GrowthChart, build_growth_chart};
use crate::draw::{Seg, put, seg, segs_width};
use crate::fmt::{clip, pad_start, sanitize};
use crate::theme::{self, fg};
use peektrace_core::sessions::model::AnalyzedSession;

const Y_GUTTER: usize = 8;
const LEGEND_LABEL_W: usize = 16;

/// Cached chart for one `(generation, width, height)`.
#[derive(Debug, Default)]
pub struct Growth {
    chart: Option<(u64, usize, usize, GrowthChart)>,
}

fn legend_rows(c: &GrowthChart, w: usize) -> Vec<Vec<Seg>> {
    let mut rows: Vec<Vec<Seg>> = vec![Vec::new()];
    for e in &c.legend {
        let item = vec![
            seg("■", fg(e.color)),
            seg(
                format!(" {}", clip(&sanitize(&e.label), LEGEND_LABEL_W)),
                fg(theme::TEXT_DIM),
            ),
        ];
        let iw = segs_width(&item);
        let row = rows.last().map_or(0, |r| segs_width(r));
        if row > 0 && row + 2 + iw > w {
            rows.push(Vec::new());
        }
        if let Some(r) = rows.last_mut() {
            if !r.is_empty() {
                r.push(seg("  ", Style::new()));
            }
            r.extend(item);
        }
    }
    rows
}

impl Growth {
    pub fn reset(&mut self) {
        self.chart = None;
    }

    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        s: &AnalyzedSession,
        generation: u64,
        term: Rect,
    ) {
        let fit_w = usize::from(area.width).saturating_sub(Y_GUTTER + 1).max(1);
        let plot_w = usize::from(term.width)
            .saturating_sub(52 + Y_GUTTER)
            .max(20)
            .min(fit_w);
        let ts_h = usize::from(term.height).saturating_sub(20).clamp(6, 14);
        let probe = build_cached(&mut self.chart, s, generation, plot_w, ts_h);
        if probe.empty {
            put(
                buf,
                area.x,
                area.y,
                area.width,
                &[seg("No turns with usage to plot.", fg(theme::TEXT_FAINT))],
            );
            return;
        }
        let legend = legend_rows(probe, usize::from(area.width)).len();
        let fit_h = usize::from(area.height).saturating_sub(6 + legend).max(3);
        let plot_h = ts_h.min(fit_h);
        let chart = build_cached(&mut self.chart, s, generation, plot_w, plot_h).clone();
        draw_chart(buf, area, &chart);
    }
}

fn build_cached<'a>(
    slot: &'a mut Option<(u64, usize, usize, GrowthChart)>,
    s: &AnalyzedSession,
    generation: u64,
    w: usize,
    h: usize,
) -> &'a GrowthChart {
    let fresh = slot
        .as_ref()
        .is_none_or(|(g, cw, ch, _)| (*g, *cw, *ch) != (generation, w, h));
    if fresh {
        *slot = Some((generation, w, h, build_growth_chart(s, w, h)));
    }
    &slot
        .as_ref()
        .unwrap_or_else(|| unreachable!("just filled"))
        .3
}

fn draw_chart(buf: &mut Buffer, area: Rect, c: &GrowthChart) {
    let bottom = area.bottom();
    let mut y = area.y;
    let line = |buf: &mut Buffer, y: &mut u16, segs: &[Seg]| {
        if *y < bottom {
            put(buf, area.x, *y, area.width, segs);
        }
        *y += 1;
    };
    let content_w = c.width + Y_GUTTER + 1;
    let header = clip(
        &format!("{} · {}", c.header, c.threshold_label),
        content_w.saturating_sub(18).max(10),
    );
    line(
        buf,
        &mut y,
        &[
            seg("Context growth ", fg(theme::ACCENT)),
            seg(header, fg(theme::TEXT_DIM)),
        ],
    );
    line(buf, &mut y, &[]);
    for (r, cells) in c.rows.iter().enumerate() {
        let label = c
            .y_ticks
            .iter()
            .find(|(row, _)| *row == r)
            .map_or("", |(_, l)| l.as_str());
        let mut segs = vec![
            seg(pad_start(label, Y_GUTTER), fg(theme::TEXT_FAINT)),
            seg(" ", Style::new()),
        ];
        let on = c.threshold_row == Some(r);
        for cell in cells {
            segs.push(match cell {
                Some(color) => seg("█", fg(*color)),
                None if on => seg("╌", fg(theme::WARN)),
                None => seg(" ", fg(theme::BORDER)),
            });
        }
        line(buf, &mut y, &segs);
    }
    let mut marks = vec![seg(" ".repeat(Y_GUTTER + 1), Style::new())];
    for col in 0..c.width {
        marks.push(if c.peak_col == Some(col) {
            seg("◆", fg(theme::TEXT))
        } else if c.compaction_cols.contains(&col) {
            seg("│", fg(theme::WARN))
        } else {
            seg(" ", Style::new())
        });
    }
    line(buf, &mut y, &marks);
    let mut axis = vec![' '; c.width];
    for (col, label) in &c.x_ticks {
        for (i, ch) in label.chars().enumerate() {
            if let Some(cell) = axis.get_mut(col + i) {
                *cell = ch;
            }
        }
    }
    line(
        buf,
        &mut y,
        &[
            seg(pad_start("turn", Y_GUTTER), fg(theme::TEXT_FAINT)),
            seg(" ", Style::new()),
            seg(axis.into_iter().collect::<String>(), fg(theme::TEXT_DIM)),
        ],
    );
    line(buf, &mut y, &[]);
    for row in legend_rows(c, usize::from(area.width)) {
        line(buf, &mut y, &row);
    }
}
