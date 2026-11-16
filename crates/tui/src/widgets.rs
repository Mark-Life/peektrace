//! Small composite pieces every screen uses: key hints, bars, buttons,
//! labelled fields and the one-line states.

use ratatui::style::{Modifier, Style};

use crate::draw::{Seg, seg};
use crate::fmt::{clip, pad_end};
use crate::theme::{self, fg};

/// `key label` pairs, two cells apart.
pub fn key_hints(hints: &[(&str, &str)]) -> Vec<Seg> {
    let mut out = Vec::with_capacity(hints.len() * 3);
    for (i, (k, l)) in hints.iter().enumerate() {
        if i > 0 {
            out.push(seg("  ", Style::new()));
        }
        out.push(seg(*k, fg(theme::ACCENT)));
        out.push(seg(format!(" {l}"), fg(theme::TEXT_FAINT)));
    }
    out
}

/// A one-row stacked bar: each `(weight, colour)` claims its share of
/// `width` cells; the rest is an empty track.
pub fn stacked_bar(parts: &[(f64, ratatui::style::Color)], width: usize) -> Vec<Seg> {
    let total: f64 = parts.iter().map(|(w, _)| w.max(0.0)).sum();
    let mut out = Vec::new();
    let mut used = 0usize;
    if total > 0.0 {
        for (weight, color) in parts {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "a share of a small width"
            )]
            let n =
                peektrace_core::js::num::js_round(weight.max(0.0) / total * width as f64) as usize;
            if n > 0 {
                out.push(seg("█".repeat(n), Style::new().fg(*color).bg(*color)));
                used += n;
            }
        }
    }
    if used < width {
        out.push(seg("░".repeat(width - used), fg(theme::BORDER)));
    }
    out
}

/// An inline button: ` label `, tinted when active.
pub fn button(label: &str, active: bool) -> Seg {
    let style = if active {
        Style::new().fg(theme::ON_PRIMARY).bg(theme::PRIMARY)
    } else {
        Style::new().fg(theme::ACCENT).bg(theme::PANEL_SEL)
    };
    seg(format!(" {label} "), style)
}

/// `label` padded to `label_w` (dim), then the value.
pub fn field(label: &str, value: &str, value_style: Style, label_w: usize) -> Vec<Seg> {
    vec![
        seg(pad_end(label, label_w), fg(theme::TEXT_DIM)),
        seg(value, value_style),
    ]
}

pub fn loading(label: &str) -> Vec<Seg> {
    vec![seg(label, fg(theme::TEXT_DIM))]
}

pub fn error_line(message: &str) -> Vec<Seg> {
    vec![seg(format!("✗ {}", clip(message, 200)), fg(theme::BAD))]
}

pub fn empty(label: &str) -> Vec<Seg> {
    vec![seg(label, fg(theme::TEXT_FAINT))]
}

pub fn bold(c: ratatui::style::Color) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}
