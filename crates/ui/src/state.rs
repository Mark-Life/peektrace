//! App state and its transitions. Loads go out as [`Request`]s that the app
//! layer hands to a [`DataSource`]; replies come back through
//! [`Ui::apply_listing`] and [`Ui::apply_analysis`] and change nothing when
//! a newer request superseded them.

use std::collections::HashSet;
use std::sync::Arc;

use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{AgentId, WatchVersions};
use rune::{AppHandle, TextEdit, Tx};

use crate::chart::geom::{Growth, growth};
use crate::model::chat::{
    ChatNode, build_chat_plan, chat_ids_of, chat_node_keys, plan_index_of_turn,
};
use crate::model::list::{self, Filters};
use crate::model::settle::{Settle, SettleEffect, SettleEvent, step};
use crate::model::tools::{ToolIndex, index_tools};
use crate::model::turns::{cross_event_index, turn_numbers};
use crate::source::{DataSource, LoadError};

/// The search applies this long after the last key.
pub const SEARCH_WRITE_S: f64 = 0.25;
/// Live dots re-check this often.
pub const NOW_TICK_S: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Sessions,
    Stats,
    Memory,
    Settings,
    Capabilities,
}

impl Route {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Stats => "Stats",
            Self::Memory => "Memory",
            Self::Settings => "Settings",
            Self::Capabilities => "Capabilities",
        }
    }
}

/// A load the app layer runs; `rid` names the request in its slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Sessions {
        rid: u64,
    },
    Analysis {
        rid: u64,
        id: String,
        parent: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub id: String,
    pub parent: Option<String>,
}

/// An analysis and everything the detail view derives from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub id: String,
    pub a: AnalyzedSession,
    pub turns: Vec<usize>,
    pub cross: Option<usize>,
    pub plan: Vec<ChatNode>,
    pub keys: Vec<String>,
    pub ids: Vec<String>,
    pub tools: ToolIndex,
    pub growth: Option<Growth>,
    /// Bumps on every load, so memoised rows rebuild.
    pub rev: u64,
}

impl Detail {
    pub fn new(id: String, a: AnalyzedSession, rev: u64) -> Self {
        let turns = turn_numbers(&a);
        let cross = cross_event_index(&a);
        let rows: Vec<usize> = (0..a.events.len()).collect();
        let plan = build_chat_plan(&a.events, &rows, &turns, cross);
        let keys = chat_node_keys(&plan);
        let ids = chat_ids_of(&a.events, &rows);
        let tools = index_tools(&a.events);
        let growth = growth(&a);
        Self {
            id,
            a,
            turns,
            cross,
            plan,
            keys,
            ids,
            tools,
            growth,
            rev,
        }
    }
}

/// The session rail: the listing, its order and what the filters keep.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rail {
    pub listing: Option<SessionListingView>,
    /// Header indexes, newest first.
    pub order: Vec<usize>,
    /// Header indexes the filters keep, in order.
    pub rows: Vec<usize>,
    pub filters: Filters,
    pub rev: u64,
}

impl Rail {
    pub fn refilter(&mut self) {
        let headers = self.listing.as_ref().map_or(&[][..], |l| &l.headers);
        self.rows = list::visible_rows(headers, &self.order, &self.filters);
        self.rev += 1;
    }

    pub fn header(&self, i: usize) -> Option<&peektrace_protocol::view::SessionHeader> {
        self.listing.as_ref()?.headers.get(i)
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.listing
            .as_ref()?
            .headers
            .iter()
            .position(|h| h.id == id)
    }
}

/// How the app reaches its data, absent in unit tests.
pub struct Io {
    pub source: Arc<dyn DataSource>,
    pub handle: AppHandle<Ui>,
}

#[expect(clippy::struct_excessive_bools, reason = "independent view flags")]
pub struct Ui {
    pub route: Route,
    pub selected: Option<Selected>,
    pub rail: Rail,
    pub detail: Option<Detail>,
    pub detail_error: Option<LoadError>,
    pub list_error: Option<LoadError>,
    pub query: TextEdit,
    pub query_due: Option<f64>,
    pub agent_open: bool,
    /// Open collapse ids of the transcript (`chat:<pos>`), per session.
    pub open: HashSet<String>,
    /// Runs whose folded chips are shown, by first position.
    pub runs_shown: HashSet<usize>,
    /// Bubbles shown in full past the clamp, and raw bodies.
    pub unclamped: HashSet<String>,
    pub raw: HashSet<String>,
    /// Big outputs shown past the cut.
    pub full: HashSet<String>,
    pub hover: Option<usize>,
    pub cursor: Option<String>,
    pub nav_collapsed: bool,
    /// `None` follows the system.
    pub dark: Option<bool>,
    pub now_ms: f64,
    pub now_fixed: bool,
    pub settle: Settle,
    pub paused: bool,
    pub versions: Option<WatchVersions>,
    pub syncing: u32,
    pub pending: Vec<Request>,
    pub sessions_rid: u64,
    pub analysis_rid: u64,
    pub io: Option<Io>,
}

impl Ui {
    pub fn new(io: Option<Io>, now_ms: f64, now_fixed: bool) -> Self {
        Self {
            route: Route::Sessions,
            selected: None,
            rail: Rail::default(),
            detail: None,
            detail_error: None,
            list_error: None,
            query: TextEdit::new(""),
            query_due: None,
            agent_open: false,
            open: HashSet::new(),
            runs_shown: HashSet::new(),
            unclamped: HashSet::new(),
            raw: HashSet::new(),
            full: HashSet::new(),
            hover: None,
            cursor: None,
            nav_collapsed: false,
            dark: None,
            now_ms,
            now_fixed,
            settle: Settle::Idle,
            paused: false,
            versions: None,
            syncing: 0,
            pending: Vec::new(),
            sessions_rid: 0,
            analysis_rid: 0,
            io,
        }
    }

    pub fn request_sessions(&mut self) {
        self.sessions_rid += 1;
        self.pending.push(Request::Sessions {
            rid: self.sessions_rid,
        });
    }

    pub fn request_analysis(&mut self) {
        let Some(sel) = self.selected.clone() else {
            return;
        };
        self.analysis_rid += 1;
        self.pending.push(Request::Analysis {
            rid: self.analysis_rid,
            id: sel.id,
            parent: sel.parent,
        });
    }

    /// Land a listing; a reply for an older request changes nothing.
    pub fn apply_listing(
        &mut self,
        rid: u64,
        result: Result<SessionListingView, LoadError>,
    ) -> bool {
        if rid != self.sessions_rid {
            return false;
        }
        match result {
            Ok(listing) => {
                self.rail.order = list::sort_by_recent(&listing.headers, self.now_ms);
                self.rail.listing = Some(listing);
                self.list_error = None;
                self.rail.refilter();
            }
            Err(e) => self.list_error = Some(e),
        }
        true
    }

    /// Land an analysis for the selected session. Open rows stay when the
    /// same session reloads; a failure keeps the old analysis on screen.
    pub fn apply_analysis(&mut self, rid: u64, result: Result<AnalyzedSession, LoadError>) -> bool {
        if rid != self.analysis_rid {
            return false;
        }
        let Some(sel) = &self.selected else {
            return false;
        };
        match result {
            Ok(a) => {
                let rev = self.detail.as_ref().map_or(0, |d| d.rev + 1);
                self.detail = Some(Detail::new(sel.id.clone(), a, rev));
                self.detail_error = None;
            }
            Err(e) => self.detail_error = Some(e),
        }
        true
    }

    /// Open a session: reset per-session view state and load it. A session
    /// hidden by the filters drops the filters that hide it.
    pub fn select(&mut self, id: &str, parent: Option<String>) {
        let same = self
            .selected
            .as_ref()
            .is_some_and(|s| s.id == id && s.parent == parent);
        self.route = Route::Sessions;
        self.cursor = Some(id.to_owned());
        if same {
            return;
        }
        self.selected = Some(Selected {
            id: id.to_owned(),
            parent,
        });
        self.detail = None;
        self.detail_error = None;
        self.open.clear();
        self.runs_shown.clear();
        self.unclamped.clear();
        self.raw.clear();
        self.full.clear();
        self.hover = None;
        if let Some(h) = self.rail.find(id).and_then(|i| self.rail.header(i)) {
            let hiding = list::hiding_filters(h, &self.rail.filters);
            if !hiding.is_empty() {
                self.rail.filters = hiding
                    .into_iter()
                    .fold(self.rail.filters.clone(), Filters::without);
                if hiding_query(&self.rail.filters, &self.query) {
                    self.query = TextEdit::new("");
                }
                self.rail.refilter();
            }
        }
        self.request_analysis();
    }

    pub fn set_agent(&mut self, agent: Option<AgentId>) {
        self.rail.filters.agent = agent;
        self.agent_open = false;
        self.rail.refilter();
    }

    /// The query box changed at `clock` seconds; apply after a pause.
    pub fn query_edited(&mut self, clock: f64, tx: &mut Tx) {
        self.query_due = Some(clock + SEARCH_WRITE_S);
        tx.wake_at(clock + SEARCH_WRITE_S);
    }

    pub fn apply_query(&mut self) {
        self.query_due = None;
        if self.rail.filters.query != self.query.text {
            self.rail.filters.query.clone_from(&self.query.text);
            self.rail.refilter();
        }
    }

    pub fn reset_filters(&mut self) {
        self.rail.filters = Filters::default();
        self.query = TextEdit::new("");
        self.query_due = None;
        self.rail.refilter();
    }

    pub fn active_filters(&self) -> usize {
        list::active_count(&self.rail.filters)
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.open.remove(id) {
            self.open.insert(id.to_owned());
        }
    }

    pub fn expand_all(&mut self) {
        if let Some(d) = &self.detail {
            self.open.extend(d.ids.iter().cloned());
        }
    }

    pub fn collapse_all(&mut self) {
        self.open.clear();
        self.runs_shown.clear();
    }

    pub fn all_open(&self) -> bool {
        self.detail
            .as_ref()
            .is_some_and(|d| !d.ids.is_empty() && d.ids.iter().all(|i| self.open.contains(i)))
    }

    /// The history row key that starts turn `n`.
    pub fn turn_row(&self, n: usize) -> Option<&str> {
        let d = self.detail.as_ref()?;
        plan_index_of_turn(&d.plan, n).map(|i| d.keys[i].as_str())
    }

    /// Watch versions arrived at `clock` seconds; a sessions bump starts
    /// or extends the settle timer.
    pub fn on_versions(&mut self, v: WatchVersions, clock: f64, tx: &mut Tx) {
        let bumped = self.versions.is_some_and(|old| v.sessions > old.sessions);
        self.versions = Some(v);
        if bumped
            && let SettleEffect::WakeAt(t) = step(
                &mut self.settle,
                SettleEvent::Bump {
                    now: clock * 1000.0,
                },
                self.paused,
            )
        {
            tx.wake_at(t / 1000.0);
        }
    }

    /// Timers: the search pause, the settle deadline, the live-dot clock.
    /// True when visible state changed.
    pub fn tick(&mut self, clock: f64, now_ms: f64, tx: &mut Tx) -> bool {
        let mut changed = false;
        if self.query_due.is_some_and(|due| clock >= due) {
            self.apply_query();
            changed = true;
        }
        match step(
            &mut self.settle,
            SettleEvent::Tick {
                now: clock * 1000.0,
            },
            self.paused,
        ) {
            SettleEffect::Fire => {
                self.request_sessions();
                self.request_analysis();
                changed = true;
            }
            SettleEffect::WakeAt(t) => tx.wake_at(t / 1000.0),
            SettleEffect::None => {}
        }
        if !self.now_fixed && now_ms - self.now_ms >= NOW_TICK_S * 1000.0 {
            self.now_ms = now_ms;
            changed = true;
        }
        changed
    }

    pub fn title(&self) -> String {
        let name = self
            .detail
            .as_ref()
            .map(|d| crate::model::labels::analyzed_title(&d.a))
            .or_else(|| {
                let sel = self.selected.as_ref()?;
                self.rail
                    .find(&sel.id)
                    .and_then(|i| self.rail.header(i))
                    .map(list::title_of)
            });
        name.map_or_else(|| "Peektrace".to_owned(), |n| format!("{n} · Peektrace"))
    }
}

fn hiding_query(f: &Filters, q: &TextEdit) -> bool {
    f.query.is_empty() && !q.text.is_empty()
}

#[cfg(test)]
mod tests;
