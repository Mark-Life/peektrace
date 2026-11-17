//! The session rail: the filter and facet lines, then the visible cards.

use peektrace_core::model::SessionHeader;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::{CARD_H, Focus, State, marks, title_of};
use crate::draw::{frame, inset, panel, put, put_lr, seg, segs_width};
use crate::fmt::{clip, fmt_started, sanitize};
use crate::screens::Hit;
use crate::theme::{self, agent_label, fg};
use crate::widgets::{empty, error_line, loading};

impl State {
    pub(super) fn draw_rail(&mut self, buf: &mut Buffer, rail: Rect, hits: &mut Vec<(Rect, Hit)>) {
        let rows = self.filtered();
        let total = self.headers().len();
        let title = format!("Sessions ({}/{total})", rows.len());
        let inner = panel(buf, rail, &title, self.focus == Focus::List, 1);
        if inner.height == 0 {
            return;
        }
        let w = inner.width;
        let slash = fg(if self.filtering {
            theme::PRIMARY
        } else {
            theme::TEXT_FAINT
        });
        let mut line = vec![seg("/ ", slash)];
        if self.filtering {
            line.extend(
                self.filter
                    .view("filter title / id…", usize::from(w.saturating_sub(2))),
            );
        } else if self.filter.text.is_empty() {
            line.push(seg("filter (press /)", fg(theme::TEXT_FAINT)));
        } else {
            line.push(seg(self.filter.text.clone(), fg(theme::TEXT)));
        }
        put(buf, inner.x, inner.y, w, &line);
        let facet = self.agent.map_or_else(
            || "agent: all · a to cycle".to_owned(),
            |a| format!("agent: {} · a to cycle", agent_label(a)),
        );
        put(
            buf,
            inner.x,
            inner.y + 1,
            w,
            &[seg(facet, fg(theme::TEXT_FAINT))],
        );
        let mut y = inner.y + 2;
        let mut status = Vec::new();
        if self.list.loading() && total == 0 {
            status.push(loading("Loading…"));
        }
        if let Some(e) = &self.list.error {
            status.push(error_line(e));
        }
        if !self.list.loading() && self.list.error.is_none() && total == 0 {
            status.push(empty("No sessions found."));
        }
        if total > 0 && rows.is_empty() {
            status.push(empty("No sessions match the filter."));
        }
        for s in &status {
            if y < inner.bottom() {
                put(buf, inner.x, y, w, s);
            }
            y += 1;
        }
        y += 1;
        if y >= inner.bottom() {
            return;
        }
        let region = Rect::new(inner.x, y, w, inner.bottom() - y);
        hits.push((region, Hit::Rail));
        let fits = (usize::from(region.height) / CARD_H).max(1);
        let sel = self.sel.at(rows.len());
        if self.follow {
            if sel < self.rail_top {
                self.rail_top = sel;
            } else if sel >= self.rail_top + fits {
                self.rail_top = sel + 1 - fits;
            }
            self.follow = false;
        }
        self.rail_top = self.rail_top.min(rows.len().saturating_sub(1));
        let card_w = w.saturating_sub(1);
        let mut cy = region.y;
        for (pos, i) in rows.iter().enumerate().skip(self.rail_top) {
            if cy >= region.bottom() {
                break;
            }
            let h = u16::try_from(CARD_H).unwrap_or(4).min(region.bottom() - cy);
            let r = Rect::new(region.x, cy, card_w, h);
            Self::card(buf, r, &self.headers()[*i], pos == sel);
            hits.push((r, Hit::Card(pos)));
            cy += h;
        }
    }

    fn card(buf: &mut Buffer, r: Rect, h: &SessionHeader, selected: bool) {
        let full = Rect::new(r.x, r.y, r.width, u16::try_from(CARD_H).unwrap_or(4));
        let mut scratch = Buffer::empty(full);
        let (border, title_style) = if selected {
            (fg(theme::BORDER_ACTIVE), fg(theme::PRIMARY))
        } else {
            (fg(theme::BORDER), fg(theme::TEXT_DIM))
        };
        if selected {
            scratch.set_style(full, Style::new().bg(theme::PANEL_SEL));
        }
        let inner = inset(frame(&mut scratch, full, None, border, title_style), 1, 0);
        let w = usize::from(inner.width);
        let date = seg(fmt_started(h.started_at.as_deref()), fg(theme::TEXT_FAINT));
        let title = seg(
            sanitize(&title_of(h)),
            fg(if selected {
                theme::PRIMARY
            } else {
                theme::TEXT
            }),
        );
        let title_w = w.saturating_sub(segs_width(std::slice::from_ref(&date)) + 1);
        let title = seg(clip(&title.0, title_w), title.1);
        put_lr(
            &mut scratch,
            inner.x,
            inner.y,
            inner.width,
            &[title],
            &[date],
        );
        let count = seg(format!("{} msgs", h.message_count), fg(theme::TEXT_FAINT));
        let mut left = marks::glyphs(h);
        left.push(seg(" ", Style::new()));
        if h.worktree.is_some() {
            left.push(seg(format!("{} ", marks::WORKTREE), fg(theme::ACCENT)));
        }
        let room =
            w.saturating_sub(segs_width(&left) + segs_width(std::slice::from_ref(&count)) + 1);
        let (project, branch) =
            marks::fit_place(&marks::project_label(h), marks::branch(h).as_deref(), room);
        left.push(seg(project, fg(theme::TEXT_DIM)));
        if let Some(b) = branch {
            left.push(seg(marks::SEP, fg(theme::TEXT_FAINT)));
            left.push(seg(b, fg(theme::TEXT_FAINT)));
        }
        put_lr(
            &mut scratch,
            inner.x,
            inner.y + 1,
            inner.width,
            &left,
            &[count],
        );
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if let (Some(src), Some(dst)) =
                    (scratch.cell((x, y)).cloned(), buf.cell_mut((x, y)))
                {
                    *dst = src;
                }
            }
        }
    }
}
