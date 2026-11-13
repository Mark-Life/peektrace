//! The analysis pane: summary card (or its one-line form), the view
//! buttons, and the history or growth view below.

use std::fmt::Write as _;

use peektrace_core::js::num::to_js_string;
use peektrace_core::sessions::cost::cost_summary;
use peektrace_core::sessions::model::AnalyzedSession;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::draw::{Seg, frame, inset, put_lr, seg};
use crate::fmt::{clip, fmt, fmt_k, fmt_pct, pad_end, pad_start, sanitize};
use crate::theme::{self, fg, provider_label, verdict, zone_of};
use crate::widgets::{bold, field, stacked_bar};

const LABEL_W: usize = 15;
const TOP_SLICES: usize = 5;
const BAR_MAX: usize = 56;
const MIN_LABEL_W: usize = 10;

fn title_of(s: &AnalyzedSession) -> String {
    s.parsed
        .title
        .clone()
        .unwrap_or_else(|| format!("{} session", provider_label(s.parsed.provider)))
}

fn frac(s: &AnalyzedSession) -> f64 {
    if s.context_window > 0.0 {
        s.peak_context_tokens / s.context_window
    } else {
        0.0
    }
}

fn web_counts(s: &AnalyzedSession) -> String {
    let mut out = String::new();
    if s.web_search_requests > 0.0 {
        let _ = write!(out, " · {} web search", to_js_string(s.web_search_requests));
    }
    if s.web_fetch_requests > 0.0 {
        let _ = write!(out, " · {} web fetch", to_js_string(s.web_fetch_requests));
    }
    out
}

/// One card row: left segments, and segments flush right.
type Row = (Vec<Seg>, Vec<Seg>);

/// The summary card's rows, `w` columns of content.
#[expect(clippy::too_many_lines, reason = "one row per card line, in order")]
fn card_rows(s: &AnalyzedSession, w: usize) -> Vec<Row> {
    let bar_w = w.saturating_sub(2).min(BAR_MAX);
    let (label, color) = verdict(zone_of(s));
    let rows: Vec<Vec<Seg>> = vec![
        vec![
            seg(
                clip(
                    &sanitize(&title_of(s)),
                    w.saturating_sub(label.len() + 6).max(MIN_LABEL_W),
                ),
                bold(theme::TEXT),
            ),
            seg(format!("  ● {label}"), fg(color)),
        ],
        vec![seg(
            clip(
                &format!(
                    "{} · {} · {}",
                    peektrace_core::js::units::slice(&s.parsed.session_id, 0, 8),
                    if s.parsed.models.is_empty() {
                        "—".to_owned()
                    } else {
                        s.parsed.models.join(", ")
                    },
                    s.parsed.cwd.as_deref().unwrap_or("—")
                ),
                w,
            ),
            fg(theme::TEXT_FAINT),
        )],
        vec![],
        stacked_bar(
            &[
                (s.peak_context_tokens, color),
                (
                    (s.context_window - s.peak_context_tokens).max(0.0),
                    theme::BORDER,
                ),
            ],
            bar_w,
        ),
        vec![seg(
            format!(
                "peak {} · {} of {}",
                fmt(s.peak_context_tokens),
                fmt_pct(frac(s)),
                fmt(s.context_window)
            ),
            fg(theme::TEXT_DIM),
        )],
        vec![],
    ];
    let text = fg(theme::TEXT);
    let mut rows: Vec<Row> = rows.into_iter().map(|r| (r, Vec::new())).collect();
    let left = |r: Vec<Seg>| (r, Vec::new());
    rows.push(left(field(
        "Turns",
        &format!(
            "{} · {} tools · {} out{}",
            s.turn_count,
            s.tool_call_count,
            fmt(s.total_output_tokens),
            web_counts(s)
        ),
        text,
        LABEL_W,
    )));
    rows.push(left(field(
        "System + tools",
        &fmt(s.system_overhead_tokens),
        text,
        LABEL_W,
    )));
    if let Some(cost) = &s.cost {
        let c = cost_summary(cost, provider_label(s.parsed.provider));
        let line = std::iter::once(c.total)
            .chain(c.notes)
            .collect::<Vec<_>>()
            .join(" · ");
        rows.push(left(field(
            "Est. cost",
            &clip(&line, w.saturating_sub(LABEL_W)),
            text,
            LABEL_W,
        )));
    }
    let cache = if s.peak_context_tokens > 0.0 {
        s.peak_cache_read_tokens / s.peak_context_tokens
    } else {
        0.0
    };
    let window = format!(
        "{}{}",
        fmt_k(s.context_window),
        if s.context_window_inferred {
            " (assumed)"
        } else {
            ""
        }
    );
    rows.push(left(field(
        "Cache / window",
        &format!("{} · {window}", fmt_pct(cache)),
        text,
        LABEL_W,
    )));
    rows.push(left(vec![]));
    rows.push(left(vec![seg("Budget at peak", fg(theme::ACCENT))]));
    let slices: Vec<_> = s.budget.iter().filter(|b| b.tokens > 0.0).collect();
    rows.push(left(stacked_bar(
        &slices
            .iter()
            .map(|b| (b.tokens, theme::hex(b.color)))
            .collect::<Vec<_>>(),
        bar_w,
    )));
    let mut top = slices.clone();
    top.sort_by(|a, b| {
        b.tokens
            .partial_cmp(&a.tokens)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let label_w = w.saturating_sub(26).max(MIN_LABEL_W);
    for b in top.into_iter().take(TOP_SLICES) {
        let row = vec![
            seg("■", fg(theme::hex(b.color))),
            seg(
                format!(" {}", pad_end(&clip(b.label, label_w), label_w)),
                text,
            ),
        ];
        let mut right = vec![seg(pad_start(&fmt(b.tokens), 9), fg(theme::TEXT_DIM))];
        if s.context_window > 0.0 {
            right.push(seg(
                format!("  {}", fmt_pct(b.tokens / s.context_window)),
                fg(theme::TEXT_FAINT),
            ));
        }
        rows.push((row, right));
    }
    rows
}

/// Rows of the summary card, border included.
pub fn card_height(s: &AnalyzedSession, w: usize) -> u16 {
    u16::try_from(card_rows(s, w).len() + 2).unwrap_or(u16::MAX)
}

/// Draw the bordered summary card in `area` (its full height).
pub fn draw_card(buf: &mut Buffer, area: Rect, s: &AnalyzedSession, w: usize) {
    let inner = inset(
        frame(buf, area, None, fg(theme::BORDER), Style::new()),
        1,
        0,
    );
    for (i, (l, r)) in card_rows(s, w).iter().enumerate() {
        let y = inner.y + u16::try_from(i).unwrap_or(u16::MAX);
        if y >= inner.bottom() {
            break;
        }
        put_lr(buf, inner.x, y, inner.width, l, r);
    }
}

/// The one-line summary above the growth chart, or when the card does not fit.
pub fn compact(s: &AnalyzedSession, pane_w: usize) -> Vec<Seg> {
    let (label, color) = verdict(zone_of(s));
    vec![
        seg(
            clip(
                &sanitize(&title_of(s)),
                pane_w.saturating_sub(40).max(MIN_LABEL_W),
            ),
            bold(theme::TEXT),
        ),
        seg(format!("  ● {label}"), fg(color)),
        seg(
            format!(
                "  peak {} {} · {} turns",
                fmt_k(s.peak_context_tokens),
                fmt_pct(frac(s)),
                s.turn_count
            ),
            fg(theme::TEXT_DIM),
        ),
    ]
}
