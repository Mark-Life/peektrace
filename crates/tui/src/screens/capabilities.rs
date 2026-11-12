//! Capabilities: the feature-support matrix with one dot per agent, and the
//! selected row's per-agent detail.

use peektrace_core::Agent;
use peektrace_core::capabilities::{Capability, SupportLevel, list_capabilities};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::draw::{fill_row, panel, put, seg, wrap_text};
use crate::fmt::{clip, pad_end};
use crate::list::{ListSel, window_slice};
use crate::theme::{self, agent_label, fg, level_color, level_gloss};
use crate::widgets::key_hints;

const NAME_W: usize = 30;
const COL_W: usize = 9;
const DETAIL_W: u16 = 42;
const TEXT_W: usize = DETAIL_W as usize - 4;
const LEVELS: [SupportLevel; 4] = [
    SupportLevel::Supported,
    SupportLevel::Partial,
    SupportLevel::Planned,
    SupportLevel::Unsupported,
];

#[derive(Debug, Default)]
pub struct State {
    sel: ListSel,
    loaded: bool,
}

impl State {
    pub fn enter(&mut self) {
        if !self.loaded {
            let _span = tracing::info_span!(
                parent: None,
                "tui.capabilities.list",
                peektrace.root = true,
                peektrace.kind = "tui"
            )
            .entered();
            self.loaded = true;
        }
    }

    pub fn key(&mut self, k: KeyEvent) {
        self.sel.key(k.code, list_capabilities().len());
    }

    pub fn draw(&self, buf: &mut Buffer, area: Rect) {
        if area.height < 3 {
            return;
        }
        let caps = list_capabilities();
        let main = Rect::new(area.x, area.y, area.width, area.height - 2);
        let show_detail = main.width > DETAIL_W + 20;
        let list_w = if show_detail {
            main.width - DETAIL_W - 1
        } else {
            main.width
        };
        let index = self.sel.at(caps.len());
        let inner = panel(
            buf,
            Rect::new(main.x, main.y, list_w, main.height),
            &format!("Capabilities ({})", caps.len()),
            true,
            1,
        );
        let name_w = NAME_W
            .min(usize::from(inner.width).saturating_sub(4 * COL_W))
            .max(8);
        let mut header = vec![seg(pad_end("Capability", name_w), fg(theme::TEXT_DIM))];
        for a in Agent::ALL {
            header.push(seg(pad_end(agent_label(a), COL_W), fg(theme::TEXT_DIM)));
        }
        put(buf, inner.x, inner.y, inner.width, &header);
        let visible = usize::from(area.height + 2).saturating_sub(10).max(4);
        let (start, end) = window_slice(index, caps.len(), visible);
        let mut y = inner.y + 1;
        for abs in start..end {
            let cap = &caps[abs];
            if (abs == 0 || caps[abs - 1].group != cap.group) && y < inner.bottom() {
                put(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    &[seg(cap.group.to_uppercase(), fg(theme::ACCENT))],
                );
                y += 1;
            }
            if y >= inner.bottom() {
                break;
            }
            let selected = abs == index;
            if selected {
                fill_row(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    Style::new().bg(theme::PANEL_SEL),
                );
            }
            let mark = if selected { "› " } else { "  " };
            let mut row = vec![seg(
                pad_end(&format!("{mark}{}", clip(cap.title, name_w - 2)), name_w),
                fg(if selected {
                    theme::PRIMARY
                } else {
                    theme::TEXT
                }),
            )];
            for a in Agent::ALL {
                row.push(seg(
                    pad_end("●", COL_W),
                    fg(level_color(cap.support(a).level)),
                ));
            }
            put(buf, inner.x, y, inner.width, &row);
            y += 1;
        }
        if show_detail && let Some(cap) = caps.get(index) {
            detail(
                buf,
                Rect::new(main.right() - DETAIL_W, main.y, DETAIL_W, main.height),
                cap,
            );
        }
        let mut legend = Vec::new();
        for (i, l) in LEVELS.iter().enumerate() {
            if i > 0 {
                legend.push(seg("  ", Style::new()));
            }
            legend.push(seg("■", fg(level_color(*l))));
            legend.push(seg(format!(" {}", l.as_str()), fg(theme::TEXT_FAINT)));
        }
        put(buf, area.x, area.bottom() - 2, area.width, &legend);
        let hints = key_hints(&[("↑↓/jk", "move"), ("1-5", "section"), ("q", "quit")]);
        put(buf, area.x, area.bottom() - 1, area.width, &hints);
    }
}

fn detail(buf: &mut Buffer, r: Rect, cap: &Capability) {
    let inner = panel(buf, r, "Detail", false, 1);
    let mut rows = vec![vec![seg(cap.title, fg(theme::TEXT))]];
    rows.extend(
        wrap_text(
            &clip(cap.description, TEXT_W * 2),
            TEXT_W,
            fg(theme::TEXT_DIM),
        )
        .into_iter()
        .take(2),
    );
    rows.push(vec![]);
    for a in Agent::ALL {
        let s = cap.support(a);
        rows.push(vec![
            seg(pad_end(agent_label(a), 10), fg(theme::TEXT_DIM)),
            seg(format!("● {}", s.level.as_str()), fg(level_color(s.level))),
        ]);
        let note = s.note.unwrap_or_else(|| level_gloss(s.level));
        rows.push(vec![seg(
            format!("  {}", clip(note, TEXT_W - 2)),
            fg(theme::TEXT_FAINT),
        )]);
    }
    for (i, row) in rows.iter().enumerate() {
        let y = inner.y + u16::try_from(i).unwrap_or(u16::MAX);
        if y >= inner.bottom() {
            break;
        }
        put(buf, inner.x, y, inner.width, row);
    }
}
