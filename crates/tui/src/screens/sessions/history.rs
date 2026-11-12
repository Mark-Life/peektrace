//! The history view: every non-system event as a row, expandable to its
//! decoded, highlighted body. Rows are laid out by height and only the
//! ones on screen are drawn and highlighted.

use std::collections::HashSet;

use peektrace_core::js::num::to_fixed;
use peektrace_core::sessions::labels::{
    context_edit_tag, event_badge_label, event_tokens, token_label,
};
use peektrace_core::sessions::model::{AnalyzedSession, EventKind, TimelineEvent};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use super::bodies::Bodies;
use super::decode::{turn_tags, visible_events};
use crate::draw::{fill_row, put, put_lr, seg, segs_width};
use crate::fmt::{clip, first_line, fmt, pad_start, width};
use crate::list::ListSel;
use crate::screens::Hit;
use crate::theme::{self, fg};
use crate::widgets::button;

const BADGE_MAX: usize = 20;
const SHARE_W: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Order,
    Size,
}

/// What a key did that the pane above must act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    Back,
    Handled,
    Ignored,
}

/// Open rows are keyed by position in `events`, so a re-sort keeps the same
/// rows open and an appended transcript keeps them valid.
#[derive(Debug, Default)]
pub struct History {
    pub sort: Sort,
    pub open: HashSet<usize>,
    pub expand_all: bool,
    pub sel: ListSel,
    scroll: usize,
    follow: bool,
    order: Vec<usize>,
    order_key: Option<(u64, Sort)>,
    bodies: Bodies,
}

fn kind_color(k: EventKind) -> Color {
    match k {
        EventKind::UserPrompt => theme::PRIMARY,
        EventKind::AssistantText => theme::TEXT,
        EventKind::AssistantThinking | EventKind::Summary => theme::ACCENT,
        EventKind::ToolCall | EventKind::SystemPrompt => theme::INFO,
        EventKind::ToolResult => theme::GOOD,
        EventKind::Attachment | EventKind::Compaction => theme::WARN,
        EventKind::System | EventKind::Meta => theme::TEXT_FAINT,
    }
}

fn badge_color(e: &TimelineEvent) -> Color {
    if e.is_error == Some(true) {
        theme::BAD
    } else if e.kind == EventKind::UserPrompt && !e.is_user_authored() {
        theme::TEXT_FAINT
    } else {
        kind_color(e.kind)
    }
}

fn row_preview(e: &TimelineEvent) -> String {
    context_edit_tag(e).map_or_else(|| e.preview.clone(), |t| format!("[{t}] {}", e.preview))
}

impl History {
    /// Forget everything tied to the previous session.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Forget prepared bodies but keep open rows and scroll: the same
    /// session, re-read another way (redaction flipped).
    pub fn forget_bodies(&mut self) {
        self.bodies.clear();
    }

    fn order(&mut self, s: &AnalyzedSession, generation: u64) -> &[usize] {
        if self.order_key != Some((generation, self.sort)) {
            let mut rows = visible_events(s);
            if self.sort == Sort::Size {
                rows.sort_by(|a, b| {
                    let t = |i: &usize| event_tokens(&s.parsed.events[*i]).0;
                    t(b).partial_cmp(&t(a)).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            self.order = rows;
            self.order_key = Some((generation, self.sort));
        }
        &self.order
    }

    fn is_open(&self, id: usize) -> bool {
        self.expand_all || self.open.contains(&id)
    }

    fn toggle(&mut self, id: usize) {
        if self.expand_all {
            self.open = self.order.iter().copied().filter(|i| *i != id).collect();
            self.expand_all = false;
        } else if !self.open.remove(&id) {
            self.open.insert(id);
        }
    }

    pub fn toggle_sort(&mut self) {
        self.sort = match self.sort {
            Sort::Order => Sort::Size,
            Sort::Size => Sort::Order,
        };
        self.sel.index = 0;
        self.follow = true;
    }

    pub fn expand_all(&mut self) {
        self.expand_all = true;
    }

    pub fn collapse_all(&mut self) {
        self.expand_all = false;
        self.open.clear();
    }

    pub fn key(&mut self, k: KeyEvent, s: &AnalyzedSession, generation: u64) -> Out {
        let count = self.order(s, generation).len();
        match k.code {
            KeyCode::Left | KeyCode::Esc => return Out::Back,
            KeyCode::Enter => {
                if let Some(id) = self.order.get(self.sel.at(count)).copied() {
                    self.toggle(id);
                }
            }
            KeyCode::Char('s') => self.toggle_sort(),
            KeyCode::Char('E') => self.expand_all(),
            KeyCode::Char('C') => self.collapse_all(),
            code => {
                if !self.sel.key(code, count) {
                    return Out::Ignored;
                }
                self.follow = true;
            }
        }
        Out::Handled
    }

    /// Click on a row: select it and toggle it.
    pub fn click(&mut self, pos: usize) {
        self.sel.index = pos;
        if let Some(id) = self.order.get(pos).copied() {
            self.toggle(id);
        }
    }

    pub fn wheel(&mut self, down: bool) {
        self.scroll = if down {
            self.scroll + 3
        } else {
            self.scroll.saturating_sub(3)
        };
    }

    /// Draw into `area`. `term_w` sizes the preview as TS does; `hits`
    /// collects mouse targets.
    #[expect(clippy::too_many_arguments, reason = "one draw call per frame")]
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        s: &AnalyzedSession,
        generation: u64,
        redact: bool,
        term_w: u16,
        hits: &mut Vec<(Rect, Hit)>,
    ) {
        if area.height == 0 {
            return;
        }
        let count = self.order(s, generation).len();
        let head = vec![
            seg("Full history", fg(theme::ACCENT)),
            seg("  ", Style::new()),
            seg(
                format!(
                    "{count} events · {}/{} in dumb zone",
                    s.dumb_zone_turns, s.turn_count
                ),
                fg(theme::TEXT_DIM),
            ),
        ];
        let red = if redact {
            seg("redacted · r to reveal", fg(theme::TEXT_FAINT))
        } else {
            seg("⚠ Redaction OFF", fg(theme::BAD))
        };
        put_lr(buf, area.x, area.y, area.width, &head, &[red]);
        if area.height > 1 {
            self.toolbar(buf, Rect::new(area.x, area.y + 1, area.width, 1), hits);
        }
        if count == 0 {
            put(
                buf,
                area.x,
                area.y + 2,
                area.width,
                &[seg("No timeline events.", fg(theme::TEXT_FAINT))],
            );
            return;
        }
        let list = Rect::new(
            area.x,
            area.y + 3,
            area.width,
            area.height.saturating_sub(3),
        );
        hits.push((list, Hit::History));
        self.rows(buf, list, s, generation, term_w, hits);
    }

    fn toolbar(&self, buf: &mut Buffer, r: Rect, hits: &mut Vec<(Rect, Hit)>) {
        let full = [
            ("Expand all", self.expand_all, Hit::ExpandAll),
            (
                "Collapse all",
                !self.expand_all && self.open.is_empty(),
                Hit::CollapseAll,
            ),
            ("Biggest first", self.sort == Sort::Size, Hit::BiggestFirst),
        ];
        let wide: usize = full.iter().map(|(l, _, _)| width(l) + 3).sum();
        let narrow = usize::from(r.width) < wide;
        let mut x = r.x;
        for (label, active, hit) in full {
            let label = if narrow { &label[..1] } else { label };
            let b = button(label, active);
            let w = put(buf, x, r.y, r.right().saturating_sub(x), &[b]);
            hits.push((Rect::new(x, r.y, w, 1), hit));
            x += w + 1;
        }
        let hint = " enter toggle · s sort · shift+E/C all";
        if usize::from(r.right().saturating_sub(x)) >= width(hint) {
            put(
                buf,
                x,
                r.y,
                r.right() - x,
                &[seg(hint, fg(theme::TEXT_FAINT))],
            );
        }
    }

    fn rows(
        &mut self,
        buf: &mut Buffer,
        list: Rect,
        s: &AnalyzedSession,
        generation: u64,
        term_w: u16,
        hits: &mut Vec<(Rect, Hit)>,
    ) {
        let count = self.order.len();
        let sel = self.sel.at(count);
        let body_w = list.width.saturating_sub(2);
        let order = std::mem::take(&mut self.order);
        self.bodies.sync(s, generation);
        let (all, open) = (self.expand_all, &self.open);
        self.bodies.prepare(
            s,
            order.iter().copied().filter(|id| all || open.contains(id)),
        );
        let mut tops = Vec::with_capacity(count + 1);
        let mut y = 0usize;
        for id in &order {
            tops.push(y);
            let h = 1 + if self.is_open(*id) {
                self.bodies.height(s, *id, body_w)
            } else {
                0
            };
            y += h + 1;
        }
        tops.push(y);
        let view = usize::from(list.height);
        if self.follow {
            let (top, bottom) = (tops[sel], tops[sel + 1] - 1);
            if top < self.scroll {
                self.scroll = top;
            } else if bottom > self.scroll + view {
                self.scroll = (bottom - view).min(top);
            }
            self.follow = false;
        }
        self.scroll = self.scroll.min(y.saturating_sub(1).saturating_sub(view));
        let tags = turn_tags(s);
        let size = self.sort == Sort::Size;
        let preview_max = usize::from(term_w)
            .saturating_sub(82 + if size { SHARE_W } else { 0 })
            .max(16);
        let first = tops.partition_point(|t| *t < self.scroll).saturating_sub(1);
        for pos in first..count {
            let top = tops[pos];
            if top >= self.scroll + view {
                break;
            }
            let id = order[pos];
            let e = &s.parsed.events[id];
            if top >= self.scroll {
                let yy = list.y + u16::try_from(top - self.scroll).unwrap_or(0);
                if pos == sel {
                    fill_row(
                        buf,
                        list.x,
                        yy,
                        list.width,
                        Style::new().bg(theme::PANEL_SEL),
                    );
                }
                let share =
                    (size && s.context_window > 0.0).then(|| event_tokens(e).0 / s.context_window);
                Self::header(
                    buf,
                    Rect::new(list.x, yy, list.width, 1),
                    e,
                    tags.get(&e.index).copied().unwrap_or(0),
                    pos == sel,
                    preview_max,
                    share,
                );
                hits.push((Rect::new(list.x, yy, list.width, 1), Hit::HistoryRow(pos)));
            }
            if self.is_open(id) {
                let rows = self.bodies.rows(s, id, body_w);
                for (i, row) in rows.iter().enumerate() {
                    let at = top + 1 + i;
                    if at < self.scroll {
                        continue;
                    }
                    if at >= self.scroll + view {
                        break;
                    }
                    let yy = list.y + u16::try_from(at - self.scroll).unwrap_or(0);
                    put(buf, list.x + 2, yy, body_w, row);
                }
            }
        }
        self.order = order;
    }

    fn header(
        buf: &mut Buffer,
        r: Rect,
        e: &TimelineEvent,
        turn: usize,
        selected: bool,
        preview_max: usize,
        share: Option<f64>,
    ) {
        let mut right = vec![seg(
            format!(" {}", token_label(e, fmt)),
            fg(theme::TEXT_FAINT),
        )];
        if let Some(sh) = share {
            right.push(seg(
                pad_start(&format!(" {}%", to_fixed(sh * 100.0, 1)), SHARE_W),
                fg(theme::TEXT_FAINT),
            ));
        }
        let lead = vec![
            seg(format!("[t{turn}]"), fg(theme::TEXT_DIM)),
            seg(" ", Style::new()),
            seg(
                format!("[{}]", clip(&event_badge_label(e), BADGE_MAX)),
                fg(badge_color(e)),
            ),
        ];
        let room = usize::from(r.width)
            .saturating_sub(segs_width(&lead) + segs_width(&right) + 1)
            .min(preview_max);
        let mut left = lead;
        let style = fg(if selected {
            theme::PRIMARY
        } else {
            theme::TEXT
        });
        left.push(seg(
            format!(" {}", first_line(&row_preview(e), room)),
            style,
        ));
        put_lr(buf, r.x, r.y, r.width, &left, &right);
    }
}
