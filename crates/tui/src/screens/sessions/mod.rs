//! Sessions: a filterable rail of session cards beside the selected
//! session's analysis.

mod decode;
mod detail;
mod growth;
pub mod growth_chart;
mod history;

pub use decode::{Decoded, decode_body, turn_tags, visible_events};

use peektrace_core::Agent;
use peektrace_core::model::SessionHeader;
use peektrace_core::sessions::model::AnalyzedSession;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;

use self::growth::Growth;
use self::history::{History, Out};
use super::{Ctx, Hit, Query};
use crate::backend::{Reply, Request, Response};
use crate::draw::{fill_row, frame, inset, panel, put, seg};
use crate::fmt::{clip, fmt_bytes, fmt_started, sanitize};
use crate::input::{Edit, LineInput};
use crate::list::ListSel;
use crate::theme::{self, agent_color, agent_label, fg};
use crate::widgets::{button, empty, error_line, key_hints, loading};

const LIST_W: u16 = 46;
const CARD_H: usize = 4;
const CARD_CONTENT_W: usize = LIST_W as usize - 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    List,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    History,
    Growth,
}

#[derive(Debug)]
#[expect(clippy::struct_excessive_bools, reason = "independent UI flags")]
pub struct State {
    list: Query<Vec<SessionHeader>>,
    hay: Vec<String>,
    list_ver: Option<u64>,
    pub focus: Focus,
    filter: LineInput,
    pub filtering: bool,
    agent: Option<Agent>,
    pub redact: bool,
    sel: ListSel,
    rail_top: usize,
    follow: bool,
    analysis: Query<Box<AnalyzedSession>>,
    shown: Option<(String, bool)>,
    stamp: Option<(u64, String)>,
    rerun: bool,
    generation: u64,
    pub view: View,
    history: History,
    growth: Growth,
}

impl Default for State {
    fn default() -> Self {
        Self {
            list: Query::default(),
            hay: Vec::new(),
            list_ver: None,
            focus: Focus::List,
            filter: LineInput::default(),
            filtering: false,
            agent: None,
            redact: true,
            sel: ListSel::default(),
            rail_top: 0,
            follow: false,
            analysis: Query::default(),
            shown: None,
            stamp: None,
            rerun: false,
            generation: 0,
            view: View::History,
            history: History::default(),
            growth: Growth::default(),
        }
    }
}

fn title_of(h: &SessionHeader) -> String {
    h.title
        .clone()
        .unwrap_or_else(|| format!("{} session", agent_label(h.agent)))
}

impl State {
    pub const fn typing(&self) -> bool {
        self.filtering
    }

    fn headers(&self) -> &[SessionHeader] {
        self.list.data.as_deref().unwrap_or_default()
    }

    /// Indexes of the headers the facet and filter keep, newest first.
    fn filtered(&self) -> Vec<usize> {
        let needle = peektrace_core::js::text::trim(&self.filter.text).to_lowercase();
        self.headers()
            .iter()
            .enumerate()
            .filter(|(i, h)| {
                self.agent.is_none_or(|a| h.agent == a)
                    && (needle.is_empty() || self.hay[*i].contains(&needle))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn selected(&self) -> Option<&SessionHeader> {
        let rows = self.filtered();
        rows.get(self.sel.at(rows.len()))
            .map(|i| &self.headers()[*i])
    }

    fn agents(&self) -> Vec<Agent> {
        let mut out: Vec<Agent> = Vec::new();
        for h in self.headers() {
            if !out.contains(&h.agent) {
                out.push(h.agent);
            }
        }
        out.sort_by_key(|a| a.as_str());
        out
    }

    fn fetch_list(&mut self, cx: &mut Ctx<'_>) {
        self.list_ver = Some(cx.watch.sessions);
        let id = cx.request(Request::Sessions);
        self.list.start(id);
    }

    /// First visit fetches; a return refetches only when files changed.
    pub fn enter(&mut self, cx: &mut Ctx<'_>) {
        if self.list_ver != Some(cx.watch.sessions) && !self.list.loading() {
            self.fetch_list(cx);
        }
    }

    pub fn watched(&mut self, cx: &mut Ctx<'_>, active: bool) {
        if active {
            self.fetch_list(cx);
        }
    }

    fn analyze(&mut self, cx: &mut Ctx<'_>, id: &str) {
        let req = cx.request(Request::Analyze {
            id: id.to_owned(),
            redact: self.redact,
        });
        self.analysis.start(req);
        self.rerun = false;
    }

    /// Point the analysis at the selected session.
    fn sync(&mut self, cx: &mut Ctx<'_>) {
        let Some(h) = self.selected() else {
            return;
        };
        let (id, stamp) = (
            h.id.as_str().to_owned(),
            (h.size_bytes, h.updated_at.clone()),
        );
        let target = (id.clone(), self.redact);
        if self.shown.as_ref() == Some(&target) {
            return;
        }
        if self.shown.as_ref().is_none_or(|(old, _)| *old != id) {
            self.history.reset();
            self.growth.reset();
        }
        self.shown = Some(target);
        self.stamp = Some(stamp);
        self.analyze(cx, &id);
    }

    /// After a list refresh: re-run the analysis when its session grew.
    fn refresh_analysis(&mut self, cx: &mut Ctx<'_>) {
        let Some(h) = self.selected() else { return };
        let stamp = (h.size_bytes, h.updated_at.clone());
        let id = h.id.as_str().to_owned();
        if self.shown.as_ref().is_none_or(|(s, _)| *s != id) {
            return;
        }
        let nested = self
            .analysis
            .data
            .as_ref()
            .is_some_and(|a| !a.parsed.subagents.is_empty());
        if self.stamp.as_ref() == Some(&stamp) && !nested {
            return;
        }
        self.stamp = Some(stamp);
        if self.analysis.loading() {
            self.rerun = true;
        } else {
            self.analyze(cx, &id);
        }
    }

    pub fn reply(&mut self, r: Reply, cx: &mut Ctx<'_>) {
        match r.resp {
            Response::Sessions(res) => {
                let keep = self.selected().map(|h| h.id.as_str().to_owned());
                let res = res.map(|mut hs| {
                    hs.sort_by(|a, b| {
                        by_started_desc(a.started_at.as_deref(), b.started_at.as_deref())
                    });
                    hs
                });
                if !self.list.land(r.id, res) {
                    return;
                }
                self.hay = self
                    .headers()
                    .iter()
                    .map(|h| format!("{} {}", title_of(h), h.id.as_str()).to_lowercase())
                    .collect();
                if let Some(keep) = keep {
                    let rows = self.filtered();
                    if let Some(at) = rows
                        .iter()
                        .position(|i| self.headers()[*i].id.as_str() == keep)
                    {
                        self.sel.index = at;
                    }
                }
                self.sync(cx);
                self.refresh_analysis(cx);
            }
            Response::Analyze(res) => {
                if self.analysis.land(r.id, res) {
                    self.generation += 1;
                    if self.rerun
                        && let Some((id, _)) = self.shown.clone()
                    {
                        self.analyze(cx, &id);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, s: &str) {
        if self.filtering {
            self.filter.insert(s);
        }
    }

    pub fn key(&mut self, k: KeyEvent, cx: &mut Ctx<'_>) {
        if k.code == KeyCode::Tab {
            self.focus = match self.focus {
                Focus::List => Focus::Detail,
                Focus::Detail => Focus::List,
            };
            return;
        }
        if self.filtering {
            match self.filter.key(k) {
                Edit::Submit | Edit::Cancel => self.filtering = false,
                Edit::Changed => self.follow = true,
                _ => {}
            }
            self.sync(cx);
            return;
        }
        match k.code {
            KeyCode::Char('/') => {
                self.filtering = true;
                return;
            }
            KeyCode::Char('g') if self.selected().is_some() => {
                self.view = match self.view {
                    View::History => View::Growth,
                    View::Growth => View::History,
                };
                return;
            }
            _ => {}
        }
        match self.focus {
            Focus::List => self.list_key(k, cx),
            Focus::Detail => self.detail_key(k),
        }
        self.sync(cx);
    }

    fn list_key(&mut self, k: KeyEvent, cx: &mut Ctx<'_>) {
        match k.code {
            KeyCode::Enter | KeyCode::Right => self.focus = Focus::Detail,
            KeyCode::Char('r') => self.redact = !self.redact,
            KeyCode::Char('a') => {
                let agents = self.agents();
                if !agents.is_empty() {
                    let cycle: Vec<Option<Agent>> = std::iter::once(None)
                        .chain(agents.into_iter().map(Some))
                        .collect();
                    let at = cycle.iter().position(|a| *a == self.agent).unwrap_or(0);
                    self.agent = cycle[(at + 1) % cycle.len()];
                    self.sel.index = 0;
                    self.follow = true;
                }
            }
            code => {
                let n = self.filtered().len();
                if self.sel.key(code, n) {
                    self.follow = true;
                }
            }
        }
        let _ = cx;
    }

    fn detail_key(&mut self, k: KeyEvent) {
        let Some(s) = self.analysis.data.as_deref() else {
            if matches!(k.code, KeyCode::Left | KeyCode::Esc) {
                self.focus = Focus::List;
            }
            return;
        };
        match self.view {
            View::History => {
                if self.history.key(k, s, self.generation) == Out::Back {
                    self.focus = Focus::List;
                }
            }
            View::Growth => {
                if matches!(k.code, KeyCode::Left | KeyCode::Esc) {
                    self.focus = Focus::List;
                }
            }
        }
    }

    pub fn click(&mut self, hit: Hit, cx: &mut Ctx<'_>) {
        match hit {
            Hit::Card(i) => {
                self.sel.index = i;
                self.sync(cx);
            }
            Hit::HistoryRow(pos) => self.history.click(pos),
            Hit::ExpandAll => self.history.expand_all(),
            Hit::CollapseAll => self.history.collapse_all(),
            Hit::BiggestFirst => self.history.toggle_sort(),
            Hit::ViewHistory => self.view = View::History,
            Hit::ViewGrowth => self.view = View::Growth,
            _ => {}
        }
    }

    pub fn wheel(&mut self, hit: Hit, down: bool) {
        match hit {
            Hit::Rail => {
                let n = self.filtered().len();
                self.rail_top = if down {
                    (self.rail_top + 1).min(n.saturating_sub(1))
                } else {
                    self.rail_top.saturating_sub(1)
                };
            }
            Hit::History => self.history.wheel(down),
            _ => {}
        }
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, term: Rect, hits: &mut Vec<(Rect, Hit)>) {
        if area.height < 2 {
            return;
        }
        let main = Rect::new(area.x, area.y, area.width, area.height - 1);
        let rail = Rect::new(main.x, main.y, LIST_W.min(main.width), main.height);
        self.draw_rail(buf, rail, hits);
        if main.width > LIST_W + 1 {
            let pane = Rect::new(
                rail.right() + 1,
                main.y,
                main.width - LIST_W - 1,
                main.height,
            );
            self.draw_detail(buf, pane, term, hits);
        }
        let hint_row = key_hints(&[
            ("↑↓/jk", "move"),
            ("enter/→", "open"),
            ("tab", "switch"),
            ("/", "filter"),
            ("a", "agent"),
            ("r", if self.redact { "reveal" } else { "hide" }),
            ("g", "chart"),
            ("click", "select"),
            ("q", "quit"),
        ]);
        put(buf, area.x, area.bottom() - 1, area.width, &hint_row);
    }

    fn draw_rail(&mut self, buf: &mut Buffer, rail: Rect, hits: &mut Vec<(Rect, Hit)>) {
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
        let badge = format!("[{}] ", agent_label(h.agent));
        let title_w = CARD_CONTENT_W.saturating_sub(badge.chars().count()).max(6);
        let model = h
            .model
            .clone()
            .unwrap_or_else(|| peektrace_core::js::units::slice(h.id.as_str(), 0, 8));
        let sub = format!(
            "{model} · {} · {} · {} msgs",
            fmt_started(h.started_at.as_deref()),
            fmt_bytes(h.size_bytes),
            h.message_count
        );
        put(
            &mut scratch,
            inner.x,
            inner.y,
            inner.width,
            &[
                seg(badge, fg(agent_color(h.agent))),
                seg(
                    clip(&sanitize(&title_of(h)), title_w),
                    fg(if selected {
                        theme::PRIMARY
                    } else {
                        theme::TEXT
                    }),
                ),
            ],
        );
        put(
            &mut scratch,
            inner.x,
            inner.y + 1,
            inner.width,
            &[seg(
                clip(&sanitize(&sub), CARD_CONTENT_W),
                fg(theme::TEXT_FAINT),
            )],
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

    fn draw_detail(
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
        let _ = fill_row;
    }
}

/// `startedAt` descending, missing last.
fn by_started_desc(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    let (a, b) = (a.unwrap_or(""), b.unwrap_or(""));
    match (a.is_empty(), b.is_empty()) {
        _ if a == b => std::cmp::Ordering::Equal,
        (true, _) => std::cmp::Ordering::Greater,
        (_, true) => std::cmp::Ordering::Less,
        _ => b.cmp(a),
    }
}
