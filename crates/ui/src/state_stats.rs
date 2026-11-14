//! Stats state: one cached report per scope, drill pages, the open drill
//! and the screen's own toggles. Reports never follow the watch; Rescan
//! asks again. Replies land through [`Ui::apply_stats`] and
//! [`Ui::apply_drill`] and change nothing once a newer ask replaced them.
//! A pass the source skipped for another scope asks again when its scope
//! is on screen, else on the next visit.

use std::collections::HashMap;
use std::time::Duration;

use peektrace_protocol::stats::DrillView;
use peektrace_protocol::{AgentId, DrillTable};
use rune::{ClipboardContent, Tx};

use crate::model::chat::{ChatNode, collapse_id};
use crate::model::stats::{DrillAsk, StatsScope};
use crate::source::{LoadError, StatsScreen};
use crate::state::{Request, Route, Ui};

/// How long a copy button reads "Copied".
pub const COPIED_S: f64 = 1.5;

/// One load: its value, the failure of the last ask, and whether an ask is
/// in flight. A failure shows over a stale value.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot<T> {
    pub value: Option<T>,
    pub error: Option<LoadError>,
    pub loading: bool,
    pub rid: u64,
}

impl<T> Slot<T> {
    const fn asked(rid: u64) -> Self {
        Self {
            value: None,
            error: None,
            loading: true,
            rid,
        }
    }

    /// Nothing to show yet: the first ask is in flight.
    pub const fn pending(&self) -> bool {
        self.value.is_none() && self.error.is_none()
    }

    fn land(&mut self, result: Result<T, LoadError>) {
        self.loading = false;
        match result {
            Ok(v) => {
                self.value = Some(v);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

pub type DrillKey = (StatsScope, DrillAsk);

/// A copy button's state after a press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CopyState {
    /// Written; the read back that tells whether it took is on its way.
    Copying,
    /// On the clipboard; reads "Copied" until this clock.
    Copied { until: f64 },
    /// The clipboard does not hold it: the button shows the text to copy by hand.
    Failed,
}

/// Whether the clipboard read back holds `wrote`; a platform may turn `\n` into `\r\n`.
fn holds(wrote: &str, read: &str) -> bool {
    wrote.lines().eq(read.lines())
}

#[derive(Debug, Default)]
#[expect(clippy::struct_excessive_bools, reason = "independent view flags")]
pub struct StatsState {
    pub scope: StatsScope,
    pub reports: HashMap<StatsScope, Slot<StatsScreen>>,
    pub drills: HashMap<DrillKey, Slot<DrillView>>,
    /// The open drill sheet's row.
    pub drill: Option<(DrillTable, String)>,
    /// `(table:key, offset)`; another row reads offset 0.
    pub page: (String, u32),
    /// When the first ask for a scope went out, in clock seconds.
    pub first_pass: HashMap<StatsScope, f64>,
    pub show_all: bool,
    pub open_cause: Option<String>,
    pub caveats_open: bool,
    pub days_open: bool,
    pub agent_open: bool,
    /// The open `×N` sample list, by its chip key.
    pub targets_open: Option<String>,
    /// The last pressed copy button and its state.
    pub copy: Option<(String, CopyState)>,
    /// The clock as the last tick saw it.
    pub clock: f64,
    pub rid: u64,
}

fn drill_id(table: DrillTable, key: &str) -> String {
    format!("{}:{key}", crate::model::stats::rows::table_name(table))
}

impl StatsState {
    pub fn report(&self) -> Option<&Slot<StatsScreen>> {
        self.reports.get(&self.scope)
    }

    pub fn screen(&self) -> Option<&StatsScreen> {
        self.report().and_then(|s| s.value.as_ref())
    }

    /// The page the open drill shows.
    pub fn drill_ask(&self) -> Option<DrillAsk> {
        let (table, key) = self.drill.clone()?;
        let offset = if self.page.0 == drill_id(table, &key) {
            self.page.1
        } else {
            0
        };
        Some(DrillAsk { table, key, offset })
    }

    pub fn drill_slot(&self) -> Option<&Slot<DrillView>> {
        self.drills.get(&(self.scope, self.drill_ask()?))
    }

    /// What the copy button `id` shows: `None` while idle or still copying.
    pub fn copy_shown(&self, id: &str) -> Option<CopyState> {
        match self.copy.as_ref() {
            Some((k, CopyState::Copied { until })) if k == id && self.clock < *until => {
                Some(CopyState::Copied { until: *until })
            }
            Some((k, CopyState::Failed)) if k == id => Some(CopyState::Failed),
            _ => None,
        }
    }

    /// Whole seconds since this scope's first ask.
    pub fn elapsed(&self) -> f64 {
        self.first_pass
            .get(&self.scope)
            .map_or(0.0, |t| (self.clock - t).floor().max(0.0))
    }
}

impl Ui {
    /// Open the Stats route; a scope with no report asks for one.
    pub fn enter_stats(&mut self, clock: f64) {
        self.route = Route::Stats;
        self.stats.clock = clock;
        if !self.stats.reports.contains_key(&self.stats.scope) {
            self.stats_ask(clock);
        }
    }

    /// Leave the Stats route: the sheet closes, the report view's toggles reset.
    pub fn leave_stats(&mut self) {
        self.stats.drill = None;
        self.stats.show_all = false;
        self.stats.days_open = false;
        self.stats.agent_open = false;
        self.stats.targets_open = None;
        self.stats.copy = None;
    }

    /// Ask for the current scope's report: the first time, or again.
    pub fn stats_ask(&mut self, clock: f64) {
        let scope = self.stats.scope;
        self.stats.rid += 1;
        let rid = self.stats.rid;
        self.stats.first_pass.entry(scope).or_insert(clock);
        self.stats
            .reports
            .entry(scope)
            .and_modify(|s| {
                s.loading = true;
                s.rid = rid;
            })
            .or_insert_with(|| Slot::asked(rid));
        self.pending.push(Request::StatsReport { rid, scope });
    }

    pub fn set_stats_scope(&mut self, days: u32, agent: Option<AgentId>, clock: f64) {
        self.stats.days_open = false;
        self.stats.agent_open = false;
        let scope = StatsScope { agent, days };
        if scope == self.stats.scope {
            return;
        }
        self.stats.scope = scope;
        self.stats.drill = None;
        self.stats.targets_open = None;
        if self
            .stats
            .reports
            .get(&scope)
            .is_none_or(|s| s.value.is_none())
        {
            self.stats.show_all = false;
        }
        if !self.stats.reports.contains_key(&scope) {
            self.stats_ask(clock);
        }
    }

    pub fn apply_stats(
        &mut self,
        rid: u64,
        scope: StatsScope,
        result: Result<StatsScreen, LoadError>,
    ) -> bool {
        match self.stats.reports.get_mut(&scope) {
            Some(slot) if slot.rid == rid => {
                if !matches!(result, Err(LoadError::Superseded)) {
                    let fresh = result.is_ok();
                    slot.land(result);
                    if fresh {
                        self.drop_drills(scope);
                    }
                } else if scope == self.stats.scope {
                    self.stats_ask(self.stats.clock);
                } else if slot.pending() {
                    // A later visit asks again and counts from then.
                    self.stats.reports.remove(&scope);
                    self.stats.first_pass.remove(&scope);
                } else {
                    slot.loading = false;
                }
                true
            }
            _ => false,
        }
    }

    /// A new pass may see new calls, so the scope's cached drill pages go;
    /// an open drill asks again from its first page.
    fn drop_drills(&mut self, scope: StatsScope) {
        self.stats.drills.retain(|(s, _), _| *s != scope);
        if scope == self.stats.scope && self.stats.drill.is_some() {
            self.stats.page = (String::new(), 0);
            self.ensure_drill();
        }
    }

    /// Open the drill sheet on one row and ask for its page when not cached.
    pub fn open_drill(&mut self, table: DrillTable, key: &str) {
        self.stats.targets_open = None;
        self.stats.drill = Some((table, key.to_owned()));
        self.ensure_drill();
    }

    pub fn close_drill(&mut self) {
        self.stats.drill = None;
    }

    /// Show another page of the open drill; it replaces the shown one.
    pub fn drill_page(&mut self, offset: u32) {
        let Some((table, key)) = self.stats.drill.clone() else {
            return;
        };
        self.stats.page = (drill_id(table, &key), offset);
        self.ensure_drill();
    }

    fn ensure_drill(&mut self) {
        let scope = self.stats.scope;
        let Some(ask) = self.stats.drill_ask() else {
            return;
        };
        if !self.stats.drills.contains_key(&(scope, ask.clone())) {
            self.drill_ask(scope, ask);
        }
    }

    /// Ask for the open drill page again.
    pub fn drill_retry(&mut self) {
        if let Some(ask) = self.stats.drill_ask() {
            self.drill_ask(self.stats.scope, ask);
        }
    }

    fn drill_ask(&mut self, scope: StatsScope, ask: DrillAsk) {
        self.stats.rid += 1;
        let rid = self.stats.rid;
        self.stats
            .drills
            .entry((scope, ask.clone()))
            .and_modify(|s| {
                s.loading = true;
                s.rid = rid;
            })
            .or_insert_with(|| Slot::asked(rid));
        self.pending.push(Request::StatsDrill { rid, scope, ask });
    }

    pub fn apply_drill(
        &mut self,
        rid: u64,
        scope: StatsScope,
        ask: DrillAsk,
        result: Result<DrillView, LoadError>,
    ) -> bool {
        let shown = scope == self.stats.scope && self.stats.drill_ask().as_ref() == Some(&ask);
        let key = (scope, ask);
        match self.stats.drills.get_mut(&key) {
            Some(slot) if slot.rid == rid => {
                if !matches!(result, Err(LoadError::Superseded)) {
                    slot.land(result);
                } else if shown {
                    self.drill_ask(key.0, key.1);
                } else if slot.pending() {
                    self.stats.drills.remove(&key);
                } else {
                    slot.loading = false;
                }
                true
            }
            _ => false,
        }
    }

    pub fn toggle_cause(&mut self, key: &str) {
        let open = self.stats.open_cause.as_deref() == Some(key);
        self.stats.open_cause = (!open).then(|| key.to_owned());
    }

    /// Copy `text`, then let the button `id` say whether it took. A plain
    /// Rune copy reports nothing, so the native app reads the clipboard
    /// back; a browser would prompt for that read, so the web trusts it.
    pub fn copy_text(&mut self, id: String, text: String, tx: &mut Tx) {
        self.stats.copy = Some((id.clone(), CopyState::Copying));
        if cfg!(target_arch = "wasm32") {
            tx.copy(text);
            self.copy_landed(&id, true, tx);
            return;
        }
        tx.copy(text.clone());
        tx.read_clipboard(move |s: &mut Self, read: &ClipboardContent, tx: &mut Tx| {
            s.copy_landed(&id, holds(&text, read.text()), tx);
        });
    }

    /// The copy of button `id` took or failed; a later press replaced it
    /// when that button is no longer copying.
    pub fn copy_landed(&mut self, id: &str, ok: bool, tx: &mut Tx) {
        if self.stats.copy != Some((id.to_owned(), CopyState::Copying)) {
            return;
        }
        let clock = tx.clock();
        self.stats.clock = clock;
        let state = if ok {
            tx.wake_at(clock + COPIED_S);
            CopyState::Copied {
                until: clock + COPIED_S,
            }
        } else {
            CopyState::Failed
        };
        self.stats.copy = Some((id.to_owned(), state));
    }

    /// Open a sample's session, through its parent for a subagent, and
    /// scroll to `call`: now when that session is already loaded, else once
    /// its analysis lands.
    pub fn open_sample(
        &mut self,
        sid: &str,
        parent: Option<String>,
        call: Option<String>,
        tx: &mut Tx,
    ) {
        self.leave_stats();
        self.select(sid, parent);
        self.pending_call = call;
        if let Some(key) = self.take_call_target() {
            tx.scroll_into_view(key);
        }
    }

    /// Open the transcript row of the pending call; its history key, for
    /// the caller to scroll to. The pending call clears once an analysis is
    /// on screen, found or not, so it never fires on a later reload.
    pub fn take_call_target(&mut self) -> Option<String> {
        let d = self.detail.as_ref()?;
        let call = self.pending_call.take()?;
        let pos = *d.tools.calls.get(call.as_str())?;
        let i = d.plan.iter().position(|n| match n {
            ChatNode::Tool { call, result } => *call == Some(pos) || *result == Some(pos),
            ChatNode::Run(items) => items.iter().any(|it| it.pos == pos),
            ChatNode::Message(p) | ChatNode::Band(p) | ChatNode::Notice(p) => *p == pos,
            ChatNode::Turn(_) | ChatNode::DumbZone => false,
        })?;
        let key = d.keys[i].clone();
        if let ChatNode::Run(items) = &d.plan[i]
            && let Some(first) = items.first()
        {
            self.runs_shown.insert(first.pos);
        }
        self.open.insert(collapse_id(pos));
        Some(key)
    }

    /// The pending-pass counter and the copied label. True when either
    /// changed what shows.
    pub fn stats_tick(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let mut changed = false;
        let pending = self.route == Route::Stats && self.stats.report().is_some_and(Slot::pending);
        if pending {
            let before = self.stats.elapsed();
            self.stats.clock = clock;
            changed |= self.stats.elapsed().total_cmp(&before).is_ne();
            tx.wake_after(Duration::from_secs(1));
        }
        if let Some((_, CopyState::Copied { until })) = self.stats.copy
            && clock >= until
        {
            self.stats.copy = None;
            self.stats.clock = clock;
            changed = true;
        }
        changed
    }
}
