//! Stats state: one cached report per scope, drill pages, the open drill
//! and the screen's own toggles. Reports never follow the watch; Rescan
//! asks again. Replies land through [`Ui::apply_stats`] and
//! [`Ui::apply_drill`] and change nothing once a newer ask replaced them.

use std::collections::HashMap;
use std::time::Duration;

use peektrace_protocol::stats::DrillView;
use peektrace_protocol::{AgentId, DrillTable};
use rune::Tx;

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
    /// The copy button that reads "Copied", and the clock when it stops.
    pub copied: Option<(String, f64)>,
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

    pub fn copied(&self, id: &str) -> bool {
        self.copied
            .as_ref()
            .is_some_and(|(k, until)| k == id && self.clock < *until)
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
                slot.land(result);
                true
            }
            _ => false,
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
        match self.stats.drills.get_mut(&(scope, ask)) {
            Some(slot) if slot.rid == rid => {
                slot.land(result);
                true
            }
            _ => false,
        }
    }

    pub fn toggle_cause(&mut self, key: &str) {
        let open = self.stats.open_cause.as_deref() == Some(key);
        self.stats.open_cause = (!open).then(|| key.to_owned());
    }

    /// Copy `text` and let the button `id` say so for a moment.
    pub fn copy_text(&mut self, id: String, text: String, tx: &mut Tx) {
        let clock = tx.clock();
        tx.copy(text);
        self.stats.clock = clock;
        self.stats.copied = Some((id, clock + COPIED_S));
        tx.wake_at(clock + COPIED_S);
    }

    /// Open a sample's session, through its parent for a subagent, and
    /// scroll to `call` once the analysis lands.
    pub fn open_sample(&mut self, sid: &str, parent: Option<String>, call: Option<String>) {
        self.leave_stats();
        self.select(sid, parent);
        self.pending_call = call;
    }

    /// Open the transcript row of the pending call; its history key, for
    /// the caller to scroll to.
    pub fn take_call_target(&mut self) -> Option<String> {
        let d = self.detail.as_ref()?;
        let call = self.pending_call.as_deref()?;
        let pos = *d.tools.calls.get(call)?;
        self.pending_call = None;
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
        if let Some((_, until)) = self.stats.copied
            && clock >= until
        {
            self.stats.copied = None;
            self.stats.clock = clock;
            changed = true;
        }
        changed
    }
}
