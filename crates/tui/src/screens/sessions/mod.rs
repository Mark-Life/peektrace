//! Sessions: a filterable rail of session cards beside the selected
//! session's analysis.

mod bodies;
mod decode;
mod detail;
mod growth;
pub mod growth_chart;
mod history;
mod rail;
mod view;

pub use decode::{Decoded, decode_body, turn_tags, visible_events};

use peektrace_core::Agent;
use peektrace_core::model::SessionHeader;
use peektrace_core::sessions::model::AnalyzedSession;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

use self::growth::Growth;
use self::history::{History, Out};
use super::{Ctx, Hit, Query};
use crate::backend::{Reply, Request, Response};
use crate::draw::put;
use crate::input::{Edit, LineInput};
use crate::list::ListSel;
use crate::theme::agent_label;
use crate::widgets::key_hints;

const LIST_W: u16 = 46;
const CARD_H: usize = 4;
const CARD_CONTENT_W: usize = LIST_W as usize - 10;
/// Least gap between list refetches that file changes drive.
const REFETCH_MS: f64 = 1000.0;

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
    /// A file change waits for a refetch, and when the last such one began.
    list_due: bool,
    list_at: Option<f64>,
    pub focus: Focus,
    filter: LineInput,
    pub filtering: bool,
    agent: Option<Agent>,
    pub redact: bool,
    sel: ListSel,
    rail_top: usize,
    follow: bool,
    analysis: Query<Box<AnalyzedSession>>,
    /// Redaction of the analysis in flight, and of the one on screen.
    asked_redact: bool,
    data_redact: bool,
    shown: Option<(String, bool)>,
    stamp: Option<(u64, String)>,
    /// The subagent files' fingerprint, and the request that asks for it.
    sub_stamp: Option<u64>,
    stamp_req: Option<u64>,
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
            list_due: false,
            list_at: None,
            focus: Focus::List,
            filter: LineInput::default(),
            filtering: false,
            agent: None,
            redact: true,
            sel: ListSel::default(),
            rail_top: 0,
            follow: false,
            analysis: Query::default(),
            asked_redact: true,
            data_redact: true,
            shown: None,
            stamp: None,
            sub_stamp: None,
            stamp_req: None,
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
        self.list_due = false;
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
            self.list_due = true;
            self.tick(cx);
        }
    }

    /// Refetch after file changes at most once per `REFETCH_MS`, and never
    /// while a fetch runs: an agent writing nonstop would otherwise re-list
    /// the whole corpus back to back.
    pub fn tick(&mut self, cx: &mut Ctx<'_>) {
        let ready = self.list_at.is_none_or(|t| cx.now - t >= REFETCH_MS);
        if self.list_due && ready && !self.list.loading() {
            self.list_at = Some(cx.now);
            self.fetch_list(cx);
        }
    }

    /// Ask for the subagent files' fingerprint, when the analysis has any.
    fn check_subagents(&mut self, cx: &mut Ctx<'_>) {
        let dirs = self
            .analysis
            .data
            .as_deref()
            .map(subagent_dirs)
            .unwrap_or_default();
        if !dirs.is_empty() {
            self.stamp_req = Some(cx.request(Request::Stamp { dirs }));
        }
    }

    fn analyze(&mut self, cx: &mut Ctx<'_>, id: &str) {
        let req = cx.request(Request::Analyze {
            id: id.to_owned(),
            redact: self.redact,
        });
        self.analysis.start(req);
        self.asked_redact = self.redact;
        self.rerun = false;
        self.sub_stamp = None;
        self.stamp_req = None;
    }

    /// Point the analysis at the selected session.
    fn sync(&mut self, cx: &mut Ctx<'_>) {
        self.sel.clamp(self.filtered().len());
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

    /// After a list refresh: re-run the analysis when its session grew. The
    /// list does not see subagent files, so a session with subagents asks
    /// for their fingerprint too.
    fn refresh_analysis(&mut self, cx: &mut Ctx<'_>) {
        let Some(h) = self.selected() else { return };
        let stamp = (h.size_bytes, h.updated_at.clone());
        let id = h.id.as_str().to_owned();
        if self.shown.as_ref().is_none_or(|(s, _)| *s != id) {
            return;
        }
        if self.stamp.as_ref() == Some(&stamp) {
            if !self.analysis.loading() {
                self.check_subagents(cx);
            }
            return;
        }
        self.stamp = Some(stamp);
        self.rerun_or_analyze(cx, &id);
    }

    fn rerun_or_analyze(&mut self, cx: &mut Ctx<'_>, id: &str) {
        if self.analysis.loading() {
            self.rerun = true;
        } else {
            self.analyze(cx, id);
        }
    }

    /// The first fingerprint after an analysis is the baseline; a later
    /// different one re-runs the analysis.
    fn stamped(&mut self, cx: &mut Ctx<'_>, stamp: u64) {
        let old = self.sub_stamp.replace(stamp);
        if old.is_some_and(|o| o != stamp)
            && let Some((id, _)) = self.shown.clone()
        {
            self.rerun_or_analyze(cx, &id);
        }
    }

    pub fn reply(&mut self, r: Reply, cx: &mut Ctx<'_>) {
        match r.resp {
            Response::Sessions(res) => {
                let keep = self.selected().map(|h| h.id.as_str().to_owned());
                let res = res.map(|mut hs| {
                    hs.sort_by(list_order);
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
                let ok = res.is_ok();
                if self.analysis.land(r.id, res) {
                    if ok {
                        self.data_redact = self.asked_redact;
                    }
                    self.generation += 1;
                    if self.rerun
                        && let Some((id, _)) = self.shown.clone()
                    {
                        self.analyze(cx, &id);
                    } else {
                        self.check_subagents(cx);
                    }
                }
            }
            Response::Stamp(Ok(stamp)) if self.stamp_req == Some(r.id) => {
                self.stamp_req = None;
                self.stamped(cx, stamp);
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, s: &str, cx: &mut Ctx<'_>) {
        if self.filtering {
            self.filter.insert(s);
            self.follow = true;
            self.sync(cx);
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
            Focus::List => self.list_key(k),
            Focus::Detail => self.detail_key(k),
        }
        self.sync(cx);
    }

    fn list_key(&mut self, k: KeyEvent) {
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
}

/// The `subagents` dirs holding a session's subagent transcripts.
fn subagent_dirs(s: &AnalyzedSession) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    for sub in &s.parsed.subagents {
        if let Some(at) = sub.path.rfind("/subagents/") {
            let dir = &sub.path[..at + "/subagents".len()];
            if !dirs.iter().any(|d| d == dir) {
                dirs.push(dir.to_owned());
            }
        }
    }
    dirs
}

/// `startedAt` descending, then `updatedAt` descending, then path. Ties
/// never fall back to directory order, which differs per filesystem.
fn list_order(a: &SessionHeader, b: &SessionHeader) -> std::cmp::Ordering {
    by_started_desc(a.started_at.as_deref(), b.started_at.as_deref())
        .then_with(|| b.updated_at.cmp(&a.updated_at))
        .then_with(|| a.path.cmp(&b.path))
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
