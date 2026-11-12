//! The analysis pane around the summary card and the lower view.

use peektrace_core::sessions::model::AnalyzedSession;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Focus, State, View, detail};
use crate::draw::{panel, put, seg};
use crate::screens::Hit;
use crate::theme::{self, fg};
use crate::widgets::{button, empty, error_line, loading};

impl State {
    pub(super) fn draw_detail(
        &mut self,
        buf: &mut Buffer,
        pane: Rect,
        term: Rect,
        hits: &mut Vec<(Rect, Hit)>,
    ) {
        if self.selected().is_none() {
            let inner = panel(buf, pane, "Analysis", false, 1);
            put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                &empty("Select a session to analyze."),
            );
            return;
        }
        let busy = self.analysis.loading() && self.analysis.data.is_some();
        let title = if busy { "Analysis ↻" } else { "Analysis" };
        let inner = panel(buf, pane, title, self.focus == Focus::Detail, 1);
        let mut y = inner.y;
        if self.analysis.loading() && self.analysis.data.is_none() {
            put(buf, inner.x, y, inner.width, &loading("Analyzing…"));
            y += 1;
        }
        if let Some(e) = &self.analysis.error {
            put(buf, inner.x, y, inner.width, &error_line(e));
            y += 1;
        }
        let Some(s) = self.analysis.data.take() else {
            if !self.analysis.loading() && self.analysis.error.is_none() {
                put(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    &empty("No analysis available."),
                );
            }
            return;
        };
        let body = Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y));
        self.layout(buf, body, &s, term, hits);
        self.analysis.data = Some(s);
    }

    fn layout(
        &mut self,
        buf: &mut Buffer,
        body: Rect,
        s: &AnalyzedSession,
        term: Rect,
        hits: &mut Vec<(Rect, Hit)>,
    ) {
        let pane_w = usize::from(term.width).saturating_sub(51).max(34);
        let text_w = pane_w
            .saturating_sub(4)
            .max(24)
            .min(usize::from(body.width.saturating_sub(4)));
        let mut y = body.y;
        let bottom = body.bottom();
        let card_h = detail::card_height(s, text_w);
        if self.view == View::History && body.height >= card_h + 9 {
            detail::draw_card(buf, Rect::new(body.x, y, body.width, card_h), s, text_w);
            y += card_h;
        } else {
            put(
                buf,
                body.x,
                y,
                body.width,
                &detail::compact(s, pane_w.min(usize::from(body.width))),
            );
            y += 1;
        }
        y += 1;
        if y < bottom {
            let h = button("History", self.view == View::History);
            let g = button("Growth", self.view == View::Growth);
            let hw = put(buf, body.x, y, body.width, &[h]);
            hits.push((Rect::new(body.x, y, hw, 1), Hit::ViewHistory));
            let gx = body.x + hw + 1;
            let gw = put(buf, gx, y, body.right().saturating_sub(gx), &[g]);
            hits.push((Rect::new(gx, y, gw, 1), Hit::ViewGrowth));
            let tx = gx + gw + 1;
            put(
                buf,
                tx,
                y,
                body.right().saturating_sub(tx),
                &[seg(" g toggle", fg(theme::TEXT_FAINT))],
            );
        }
        y += 2;
        if y >= bottom {
            return;
        }
        let region = Rect::new(body.x, y, body.width, bottom - y);
        match self.view {
            View::History => self.history.draw(
                buf,
                region,
                s,
                self.generation,
                self.redact,
                term.width,
                hits,
            ),
            View::Growth => self.growth.draw(buf, region, s, self.generation, term),
        }
    }
}
