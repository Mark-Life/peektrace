//! App state and its transitions. Loads go out as [`Request`]s that the app
//! layer hands to a [`DataSource`]; replies come back through
//! [`Ui::apply_listing`] and [`Ui::apply_analysis`] and change nothing when
//! a newer request superseded them. A watch refresh reloads the listing,
//! then the open session only when its file changed.

mod detail;
mod route;
pub mod settings;

pub use detail::{Detail, Rail, Selected};
pub use route::{Request, Route};

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::Arc;

use peektrace_protocol::view::{AnalyzedSession, SessionListingView};
use peektrace_protocol::{AgentId, WatchVersions};
use rune::{AppHandle, TextEdit, Tx};

use crate::model::chat::plan_index_of_turn;
use crate::model::list::{self, Filters};
use crate::model::settle::{Settle, SettleEffect, SettleEvent, step};
use crate::source::{DataSource, LoadError, WatchEvent};
use crate::state_memory::MemoryUi;
use crate::state_stats::StatsState;

/// The search applies this long after the last key.
pub const SEARCH_WRITE_S: f64 = 0.25;
/// Live dots re-check this often.
pub const NOW_TICK_S: f64 = 30.0;

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
    /// The system theme as the last view saw it; key handlers cannot ask.
    pub system_dark: Cell<bool>,
    pub now_ms: f64,
    pub now_fixed: bool,
    pub settle: Settle,
    pub paused: bool,
    pub versions: Option<WatchVersions>,
    /// Why live refresh misses changes, shown over the rail.
    pub watch_problems: Vec<String>,
    pub syncing: u32,
    pub pending: Vec<Request>,
    /// A watch refresh asked for the listing; the open session reloads when
    /// that listing shows its file changed.
    pub recheck_detail: bool,
    pub sessions_rid: u64,
    pub analysis_rid: u64,
    pub stats: StatsState,
    /// A stats link's call, scrolled to once its session loads.
    pub pending_call: Option<String>,
    pub settings: settings::SettingsState,
    pub caps: settings::CapsState,
    pub info: settings::InfoState,
    pub help_open: bool,
    /// Keycaps read `⌘` rather than `Ctrl+`; scenes pin it.
    pub mac: bool,
    pub toast: Option<settings::Toast>,
    /// Where the theme pick persists.
    pub prefs: rune::prefs::Prefs,
    pub memory: MemoryUi,
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
            system_dark: Cell::new(true),
            now_ms,
            now_fixed,
            settle: Settle::Idle,
            paused: false,
            versions: None,
            watch_problems: Vec::new(),
            syncing: 0,
            pending: Vec::new(),
            recheck_detail: false,
            sessions_rid: 0,
            analysis_rid: 0,
            stats: StatsState::default(),
            pending_call: None,
            settings: settings::SettingsState::default(),
            caps: settings::CapsState::default(),
            info: settings::InfoState::default(),
            help_open: false,
            mac: crate::shell::MAC,
            toast: None,
            prefs: rune::prefs::Prefs::memory(),
            memory: MemoryUi::default(),
            io,
        }
    }

    /// The theme on screen: the user's pick, else the system's.
    pub fn is_dark(&self) -> bool {
        self.dark.unwrap_or_else(|| self.system_dark.get())
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
        let recheck = std::mem::take(&mut self.recheck_detail);
        match result {
            Ok(listing) => {
                let reload = recheck && self.detail_stale(&listing);
                self.rail.order = list::sort_by_recent(&listing.headers, self.now_ms);
                self.rail.listing = Some(listing);
                self.list_error = None;
                self.rail.refilter();
                if reload {
                    self.request_analysis();
                }
            }
            Err(e) => {
                self.list_error = Some(e);
                if recheck {
                    self.request_analysis();
                }
            }
        }
        true
    }

    /// The open session needs a reload against `listing`. Subagent files are
    /// not in the listing, so a subagent, or a session that has some, always
    /// reloads.
    fn detail_stale(&self, listing: &SessionListingView) -> bool {
        self.selected.as_ref().is_some_and(|sel| {
            let old = self.rail.find(&sel.id).and_then(|i| self.rail.header(i));
            let new = listing.headers.iter().find(|h| h.id == sel.id);
            let has_subagents = self
                .detail
                .as_ref()
                .is_some_and(|d| d.id == sel.id && !d.a.subagents.is_empty());
            sel.parent.is_some()
                || has_subagents
                || self.detail_error.is_some()
                || list::changed(old, new)
        })
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
        self.pending_call = None;
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

    /// A watch event at `clock` seconds. A reconnect reloads the listing and
    /// the open session at once.
    pub fn on_watch(&mut self, ev: WatchEvent, clock: f64, tx: &mut Tx) {
        match ev {
            WatchEvent::Versions(v) => self.on_versions(v, clock, tx),
            WatchEvent::Problems(p) => self.watch_problems = p,
            WatchEvent::Reconnected => {
                self.recheck_detail = false;
                self.request_sessions();
                self.request_analysis();
                self.on_memory_reconnect();
                tx.wake_after(std::time::Duration::ZERO);
            }
        }
    }

    /// Watch versions arrived at `clock` seconds; a sessions bump starts
    /// or extends the settle timer.
    pub fn on_versions(&mut self, v: WatchVersions, clock: f64, tx: &mut Tx) {
        let bumped = self.versions.is_some_and(|old| v.sessions > old.sessions);
        self.versions = Some(v);
        self.on_memory_version(v.memory);
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
        let mut changed = self.memory_tick(clock);
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
                self.recheck_detail = true;
                changed = true;
            }
            SettleEffect::WakeAt(t) => tx.wake_at(t / 1000.0),
            SettleEffect::None => {}
        }
        if self.toast.as_ref().is_some_and(|t| clock >= t.until) {
            self.toast = None;
            changed = true;
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
