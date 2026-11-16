//! The in-process stats source and how the state takes a skipped pass.

mod common;

use std::sync::Arc;

use common::home;
use peektrace_protocol::stats::DrillView;
use peektrace_protocol::{AgentId, ApiError, DrillTable};
use peektrace_ui::model::stats::{DrillAsk, StatsScope};
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::source::{DataSource, LoadError, OnVersions, Reply, StatsScreen};
use peektrace_ui::state::{Io, Request, Ui};
use rune::App;

fn now_ms() -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    peektrace_js::time::parse_date(home::NOW, &peektrace_js::time::Utc).map_or(0.0, |ms| ms as f64)
}

fn source(home: &std::path::Path) -> InProcess {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    InProcess::for_home(dir, move |k| {
        (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
    })
    .at(now_ms())
}

fn ask(offset: u32) -> DrillAsk {
    DrillAsk {
        table: DrillTable::Cause,
        key: "cause:zsh-glob-path".into(),
        offset,
    }
}

#[test]
fn a_drill_page_turn_slices_the_first_pass() {
    let home = home::stats_home();
    let src = source(&home);
    let scope = StatsScope::default();
    let first = src.stats_drill_now(scope, &ask(0)).expect("in process");
    assert_eq!(
        first.as_ref().map(|d| (d.total, d.hits.len())),
        Ok((3.0, 3))
    );
    // No transcripts left: only the kept hits can answer the next page.
    std::fs::remove_dir_all(home.join(".claude/projects")).expect("remove transcripts");
    let next = src.stats_drill_now(scope, &ask(2)).expect("in process");
    assert_eq!(
        next.as_ref().map(|d| (d.total, d.offset, d.hits.len())),
        Ok((3.0, 2.0, 1))
    );
    assert_eq!(
        next.map(|d| d.hits[0].sample.tool_use_id.clone()),
        first.map(|d| d.hits[2].sample.tool_use_id.clone())
    );
    // A new pass over the scope drops the kept hits.
    let _ = src.stats_report_now(scope);
    let after = src.stats_drill_now(scope, &ask(0)).expect("in process");
    assert!(matches!(
        after,
        Err(LoadError::Api(ApiError::StatsRowNotFoundError { .. }))
    ));
}

struct Silent;

impl DataSource for Silent {
    fn memory(&self, _: Reply<peektrace_ui::source::memory::MemoryData>) {}
    fn memory_write(
        &self,
        _: peektrace_ui::source::memory::MemoryWrite,
        _: Reply<peektrace_ui::source::memory::MemoryDone>,
    ) {
    }
    fn sessions(&self, _: Reply<peektrace_protocol::view::SessionListingView>) {}
    fn analysis(
        &self,
        _: String,
        _: Option<String>,
        _: Reply<peektrace_protocol::view::AnalyzedSession>,
    ) {
    }
    fn watch(&self, _: OnVersions) {}
    fn capabilities(&self, _: Reply<Vec<peektrace_protocol::Capability>>) {}
    fn server_info(&self, _: Reply<peektrace_protocol::ServerInfo>) {}
    fn settings(&self, _: Reply<peektrace_protocol::SettingsResult>) {}
    fn save_settings(
        &self,
        _: peektrace_protocol::SettingsUpdate,
        _: Reply<peektrace_protocol::SettingsResult>,
    ) {
    }
    fn stats_report(&self, _: StatsScope, _: Reply<StatsScreen>) {}
    fn stats_drill(&self, _: StatsScope, _: DrillAsk, _: Reply<DrillView>) {}
}

fn ui() -> Ui {
    let io = Io {
        source: Arc::new(Silent),
        handle: App::new(peektrace_ui::spec()).handle(),
    };
    let mut s = Ui::new(Some(io), now_ms(), true);
    s.enter_stats(0.0);
    s
}

fn last_report(s: &Ui) -> (u64, StatsScope) {
    match s.pending.last() {
        Some(Request::StatsReport { rid, scope }) => (*rid, *scope),
        other => panic!("no report ask: {other:?}"),
    }
}

#[test]
fn a_skipped_pass_for_a_left_scope_asks_again_on_return() {
    let mut s = ui();
    let (rid60, d60) = last_report(&s);
    s.set_stats_scope(7, None, 1.0);
    s.pending.clear();
    assert!(s.apply_stats(rid60, d60, Err(LoadError::Superseded)));
    assert!(!s.stats.reports.contains_key(&d60));
    s.set_stats_scope(d60.days, d60.agent, 2.0);
    assert_eq!(last_report(&s).1, d60);
    assert!(s.stats.report().is_some_and(|r| r.loading));
}

#[test]
fn a_skipped_pass_for_the_shown_scope_asks_again_at_once() {
    let mut s = ui();
    let (rid60, d60) = last_report(&s);
    s.set_stats_scope(7, Some(AgentId::Claude), 1.0);
    // Back before either pass ran: the slot is still in flight, no new ask.
    s.set_stats_scope(d60.days, d60.agent, 2.0);
    s.pending.clear();
    assert!(s.apply_stats(rid60, d60, Err(LoadError::Superseded)));
    let (rid, scope) = last_report(&s);
    assert!(rid > rid60 && scope == d60);
    let slot = s.stats.report().expect("slot");
    assert!(slot.loading && slot.error.is_none() && slot.rid == rid);
}

#[test]
fn a_skipped_drill_for_a_left_scope_asks_again_when_reopened() {
    let mut s = ui();
    let d60 = s.stats.scope;
    s.open_drill(DrillTable::Cause, "cause:zsh-glob-path");
    let rid = match s.pending.last() {
        Some(Request::StatsDrill { rid, .. }) => *rid,
        other => panic!("no drill ask: {other:?}"),
    };
    s.set_stats_scope(7, None, 1.0);
    s.pending.clear();
    assert!(s.apply_drill(rid, d60, ask(0), Err(LoadError::Superseded)));
    assert!(s.stats.drills.is_empty());
    s.set_stats_scope(d60.days, d60.agent, 2.0);
    s.open_drill(DrillTable::Cause, "cause:zsh-glob-path");
    assert!(
        s.pending
            .iter()
            .any(|r| matches!(r, Request::StatsDrill { scope, .. } if *scope == d60))
    );
}
