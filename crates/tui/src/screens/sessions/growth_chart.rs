//! Pure builder for the context-growth column chart: one column per turn
//! (bucketed when turns exceed the width), each a stacked bar of budget
//! categories over a 0..window y-axis.

use peektrace_core::js::num::js_round;
use peektrace_core::sessions::model::{AnalyzedSession, BudgetKey, BudgetSlices, TurnSnapshot};
use ratatui::style::Color;

use crate::fmt::{fmt_k, fmt_pct};
use crate::theme;

/// Stacking order, floor to top.
pub const STACK_ORDER: [BudgetKey; 10] = BudgetKey::ALL;

/// A legend entry for one category with tokens in some turn.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    pub key: BudgetKey,
    pub label: String,
    pub color: Color,
}

/// The computed chart, ready to paint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrowthChart {
    /// `height` rows, top first; each cell a colour or empty.
    pub rows: Vec<Vec<Option<Color>>>,
    pub width: usize,
    pub height: usize,
    /// `(row, label)`, full window first.
    pub y_ticks: Vec<(usize, String)>,
    /// `(col, label)`.
    pub x_ticks: Vec<(usize, String)>,
    /// Row of the dumb-zone line; `None` when off the grid.
    pub threshold_row: Option<usize>,
    pub threshold_label: String,
    pub peak_col: Option<usize>,
    pub compaction_cols: Vec<usize>,
    pub legend: Vec<LegendEntry>,
    pub header: String,
    pub empty: bool,
}

struct Column {
    slices: BudgetSlices,
    turn: usize,
}

fn buckets(snaps: &[TurnSnapshot], cols: usize) -> Vec<Column> {
    let n = snaps.len();
    (0..cols)
        .filter_map(|c| {
            let start = (c * n) / cols;
            let end = (start + 1).max(((c + 1) * n) / cols).min(n);
            let rep = (start..end).max_by(|a, b| {
                snaps[*a]
                    .ctx
                    .partial_cmp(&snaps[*b].ctx)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(b.cmp(a))
            })?;
            Some(Column {
                slices: snaps[rep].slices,
                turn: rep + 1,
            })
        })
        .collect()
}

fn col_of(pos: usize, n: usize, cols: usize) -> usize {
    ((pos * cols) / n.max(1)).min(cols.saturating_sub(1))
}

/// Build the chart for `width` x `height` plot cells.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "grid arithmetic on small counts"
)]
pub fn build_growth_chart(s: &AnalyzedSession, width: usize, height: usize) -> GrowthChart {
    let win = s.context_window;
    let n = s.snapshots.len();
    let height = height.max(1);
    let cols = width.max(1);
    if win <= 0.0 || n == 0 {
        return GrowthChart {
            empty: true,
            ..GrowthChart::default()
        };
    }
    let columns = buckets(&s.snapshots, cols);
    let width = columns.len();
    let color_of = |key: BudgetKey| {
        s.budget
            .iter()
            .find(|b| b.key == key)
            .map_or(theme::TEXT_FAINT, |b| theme::hex(b.color))
    };
    let mut rows = vec![vec![None; width]; height];
    for (col, column) in columns.iter().enumerate() {
        let mut filled = 0usize;
        for key in STACK_ORDER {
            let cells = js_round(column.slices.get(key) / win * height as f64).max(0.0) as usize;
            let color = color_of(key);
            for k in 0..cells {
                let offset = filled + k;
                if offset >= height {
                    break;
                }
                rows[height - 1 - offset][col] = Some(color);
            }
            filled += cells;
        }
    }
    let raw = js_round((1.0 - s.dumb_zone_fraction) * height as f64);
    let threshold_row = (raw >= 0.0 && raw < height as f64).then_some(raw as usize);
    let y_ticks = (0..5)
        .map(|i| {
            let f = 1.0 - f64::from(i) / 4.0;
            let row = (js_round((1.0 - f) * height as f64) as usize).min(height - 1);
            (row, fmt_k(f * win))
        })
        .collect();
    let step = width.div_ceil(10).max(1);
    let x_ticks = columns
        .iter()
        .enumerate()
        .filter(|(c, _)| c % step == 0)
        .map(|(c, col)| (c, col.turn.to_string()))
        .collect();
    let peak_frac = s.peak_context_tokens / win;
    GrowthChart {
        rows,
        width,
        height,
        y_ticks,
        x_ticks,
        threshold_row,
        threshold_label: format!("DUMB ZONE {}%", js_round(s.dumb_zone_fraction * 100.0)),
        peak_col: Some(col_of(s.peak_turn_index, n, cols)),
        compaction_cols: s
            .compaction_turns
            .iter()
            .map(|t| col_of(*t, n, cols))
            .collect(),
        legend: legend(s),
        header: format!(
            "{} turns · peak {} ({}) @ turn {}",
            s.turn_count,
            fmt_k(s.peak_context_tokens),
            fmt_pct(peak_frac),
            s.peak_turn_index + 1
        ),
        empty: false,
    }
}

fn legend(s: &AnalyzedSession) -> Vec<LegendEntry> {
    STACK_ORDER
        .into_iter()
        .filter(|k| s.snapshots.iter().any(|snap| snap.slices.get(*k) > 0.0))
        .map(|key| {
            let meta = s.budget.iter().find(|b| b.key == key);
            LegendEntry {
                key,
                label: meta.map_or_else(|| key.as_str().to_owned(), |m| m.label.to_owned()),
                color: meta.map_or(theme::TEXT_FAINT, |m| theme::hex(m.color)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use peektrace_core::sessions::model::BudgetSlice;

    use super::*;

    fn snap(ctx: f64, key: BudgetKey) -> TurnSnapshot {
        let mut slices = BudgetSlices::default();
        match key {
            BudgetKey::Files => slices.files = ctx,
            BudgetKey::Thinking => slices.thinking = ctx,
            _ => slices.other = ctx,
        }
        TurnSnapshot {
            turn_index: 0,
            model: "m".into(),
            ctx,
            output_tokens: 0.0,
            cache_read_tokens: 0.0,
            slices,
            ts: None,
        }
    }

    fn session(snaps: Vec<TurnSnapshot>) -> AnalyzedSession {
        AnalyzedSession {
            context_window: 1000.0,
            dumb_zone_fraction: 0.6,
            peak_context_tokens: snaps.iter().map(|s| s.ctx).fold(0.0, f64::max),
            turn_count: snaps.len(),
            budget: STACK_ORDER
                .iter()
                .map(|k| BudgetSlice {
                    key: *k,
                    label: "L",
                    short: "s",
                    color: "#100000",
                    estimated: false,
                    note: None,
                    tokens: 1.0,
                })
                .collect(),
            snapshots: snaps,
            ..AnalyzedSession::default()
        }
    }

    fn fill(c: &GrowthChart, col: usize) -> usize {
        c.rows.iter().filter(|r| r[col].is_some()).count()
    }

    #[test]
    fn column_fill_scales_with_ctx() {
        let c = build_growth_chart(
            &session(vec![
                snap(250.0, BudgetKey::Files),
                snap(500.0, BudgetKey::Files),
            ]),
            10,
            20,
        );
        assert_eq!(c.width, 10);
        assert_eq!(fill(&c, 0), 5);
        assert_eq!(fill(&c, 9), 10);
    }

    #[test]
    fn full_window_fills_every_row() {
        let c = build_growth_chart(&session(vec![snap(1000.0, BudgetKey::Files)]), 8, 16);
        assert_eq!(fill(&c, 0), 16);
    }

    #[test]
    fn threshold_row_reflects_the_dumb_zone() {
        let mut s = session(vec![snap(500.0, BudgetKey::Files)]);
        s.dumb_zone_fraction = 0.75;
        assert_eq!(build_growth_chart(&s, 4, 20).threshold_row, Some(5));
    }

    #[test]
    fn buckets_when_snapshots_exceed_width() {
        let many = (0..50)
            .map(|i| snap(100.0 + f64::from(i), BudgetKey::Files))
            .collect();
        let c = build_growth_chart(&session(many), 10, 20);
        assert_eq!(c.width, 10);
        assert_eq!(c.rows[0].len(), 10);
    }

    #[test]
    fn legend_lists_only_nonzero_categories() {
        let c = build_growth_chart(
            &session(vec![
                snap(400.0, BudgetKey::Files),
                snap(300.0, BudgetKey::Thinking),
            ]),
            6,
            12,
        );
        let keys: Vec<BudgetKey> = c.legend.iter().map(|e| e.key).collect();
        assert_eq!(keys, vec![BudgetKey::Files, BudgetKey::Thinking]);
    }

    #[test]
    fn empty_and_zero_window_charts() {
        assert!(build_growth_chart(&session(vec![]), 10, 10).empty);
        let mut s = session(vec![snap(500.0, BudgetKey::Files)]);
        s.context_window = 0.0;
        let c = build_growth_chart(&s, 10, 10);
        assert!(c.empty);
        assert_eq!(c.rows.len(), 0);
    }

    #[test]
    fn peak_column_follows_the_peak_turn() {
        let mut s = session(
            (0..4)
                .map(|i| snap(100.0 * f64::from(i + 1), BudgetKey::Files))
                .collect(),
        );
        s.peak_turn_index = 3;
        assert_eq!(build_growth_chart(&s, 8, 10).peak_col, Some(6));
        let mut s = session((0..40).map(|_| snap(100.0, BudgetKey::Files)).collect());
        s.peak_turn_index = 20;
        assert_eq!(build_growth_chart(&s, 10, 12).peak_col, Some(5));
    }
}
