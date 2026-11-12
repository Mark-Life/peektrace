//! The views driven through Rune's `Runtime` (no GPU) on a synthetic HOME.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::home::{self, BIG_TURN, FIXTURE_ID, LONG_ID, UNTITLED_ID};
use peektrace_protocol::{AgentId, WatchVersions};
use peektrace_ui::model::chat::{ChatNode, collapse_id};
use peektrace_ui::source::DataSource;
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::state::{Io, Request, Route, Ui};
use peektrace_ui::views::session_list::row_key;
use peektrace_ui::{load_now, views};
use rune::{App, Env, Runtime, Tx};

const SIZE: (f32, f32) = (1440.0, 900.0);

fn now_ms() -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    peektrace_ui::model::jstime::parse_date(home::NOW).map_or(0.0, |ms| ms as f64)
}

fn state(home: &Path) -> Ui {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    let source: Arc<dyn DataSource> = Arc::new(InProcess::for_home(dir, move |k| {
        (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
    }));
    let handle = App::new(peektrace_ui::spec()).handle();
    let mut s = Ui::new(Some(Io { source, handle }), now_ms(), true);
    s.request_sessions();
    load_now(&mut s);
    s
}

fn runtime(s: Ui) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, SIZE, Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect()
}

fn has_text(rt: &Runtime<Ui>, needle: &str) -> bool {
    texts(rt).iter().any(|t| t.contains(needle))
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn open(s: &mut Ui, id: &str) {
    s.select(id, None);
    load_now(s);
}

fn row_keys(rt: &Runtime<Ui>) -> usize {
    rt.elements()
        .iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("s:")))
        .count()
}

#[test]
fn list_mounts_a_window_of_5000() {
    let rt = runtime(state(&home::many_home(5000)));
    assert_eq!(rt.state.rail.rows.len(), 5000);
    let mounted = row_keys(&rt);
    assert!(mounted > 0 && mounted < 60, "{mounted} rows mounted");
    assert!(has_text(&rt, "5000 sessions"));
}

#[test]
fn list_order_labels_and_live_dot() {
    let rt = runtime(state(&home::build_home()));
    let ids: Vec<&str> = rt
        .state
        .rail
        .rows
        .iter()
        .filter_map(|&i| rt.state.rail.header(i))
        .map(|h| h.id.as_str())
        .collect();
    assert_eq!(ids.first().copied(), Some(FIXTURE_ID));
    assert_eq!(ids.get(1).copied(), Some(LONG_ID));
    assert!(has_text(&rt, "Claude session"), "untitled row label");
    assert!(has_text(&rt, "parser"), "project label from cwd");
    assert!(
        !texts(&rt).iter().any(|t| t == "HEAD"),
        "detached HEAD hidden"
    );
    let live = rt
        .elements()
        .iter()
        .filter(|e| e.label.as_deref() == Some("Live"))
        .count();
    assert_eq!(live, 1);
}

#[test]
fn agent_facet_and_reset() {
    let mut rt = runtime(state(&home::build_home()));
    let all = rt.state.rail.rows.len();
    rt.update(|s, _| s.set_agent(Some(AgentId::Codex)));
    settle(&mut rt);
    let agents: Vec<AgentId> = rt
        .state
        .rail
        .rows
        .iter()
        .filter_map(|&i| rt.state.rail.header(i))
        .map(|h| h.agent)
        .collect();
    assert!(!agents.is_empty() && agents.iter().all(|a| *a == AgentId::Codex));
    assert!(has_text(&rt, "Clear 1"));
    click(&mut rt, "clear-filters");
    assert_eq!(rt.state.rail.rows.len(), all);
}

#[test]
fn search_applies_after_a_pause() {
    let mut rt = runtime(state(&home::build_home()));
    rt.update(|s, tx| {
        s.query = rune::TextEdit::new("refactor");
        s.query_edited(10.0, tx);
        assert!(!s.tick(10.1, now_ms(), tx));
    });
    assert!(rt.state.rail.rows.len() > 1);
    rt.update(|s, tx| assert!(s.tick(10.3, now_ms(), tx)));
    settle(&mut rt);
    assert_eq!(rt.state.rail.rows.len(), 1);
    assert!(has_text(&rt, "1 session"));
}

#[test]
fn pressing_a_row_opens_it() {
    let mut s = state(&home::build_home());
    s.io = None;
    let mut rt = runtime(s);
    click(&mut rt, &row_key(LONG_ID));
    assert_eq!(
        rt.state.selected.as_ref().map(|s| s.id.as_str()),
        Some(LONG_ID)
    );
    assert!(matches!(rt.state.pending.last(), Some(Request::Analysis { id, .. }) if id == LONG_ID));
}

#[test]
fn summary_card() {
    let mut s = state(&home::build_home());
    open(&mut s, LONG_ID);
    let rt = runtime(s);
    for needle in [
        "Tune the parser",
        "PEAK CONTEXT",
        "TURNS",
        "DUMB-ZONE DWELL",
        "CACHE",
        "2 compaction(s)",
        "feature/speed",
    ] {
        assert!(has_text(&rt, needle), "{needle}");
    }
    assert!(
        texts(&rt)
            .iter()
            .any(|t| ["Healthy", "Degrading", "Rotting"].contains(&t.as_str()))
    );
    let mut s = state(&home::build_home());
    open(&mut s, UNTITLED_ID);
    let rt = runtime(s);
    assert!(has_text(&rt, "Claude Code session"));
}

#[test]
fn chart_hover_shows_the_turn() {
    let mut s = state(&home::build_home());
    open(&mut s, LONG_ID);
    let mut rt = runtime(s);
    let (x, y) = rt
        .find("growth-hit")
        .unwrap_or_else(|| panic!("no hit area"));
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert!(rt.state.hover.is_some());
    assert!(
        texts(&rt)
            .iter()
            .any(|t| t.starts_with("turn ") && t.contains(" · ctx ")),
        "{:?}",
        rt.state.hover
    );
    let peak = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("growth-peak"));
    assert!(peak.is_some_and(
        |e| e.focusable && e.label.is_some_and(|l| l.contains("jump to it in history"))
    ));
}

#[test]
fn history_plan_and_expand_all() {
    let mut s = state(&home::build_home());
    open(&mut s, LONG_ID);
    let d = s.detail.as_ref().unwrap_or_else(|| panic!("no detail"));
    assert!(d.plan.contains(&ChatNode::DumbZone));
    assert!(
        d.plan
            .iter()
            .filter(|n| matches!(n, ChatNode::Turn(_)))
            .count()
            >= 120
    );
    assert!(d.plan.iter().any(|n| matches!(
        n,
        ChatNode::Tool {
            call: Some(_),
            result: Some(_)
        }
    )));
    let mut rt = runtime(s);
    scroll_to(&mut rt, "expand-all");
    click(&mut rt, "expand-all");
    assert!(rt.state.all_open());
    click(&mut rt, "collapse-all");
    assert!(rt.state.open.is_empty());
}

#[test]
fn big_output_is_cut() {
    let mut s = state(&home::build_home());
    open(&mut s, LONG_ID);
    let d = s.detail.as_ref().unwrap_or_else(|| panic!("no detail"));
    let tool = format!("toolu_{BIG_TURN:03}");
    let call =
        d.a.events
            .iter()
            .position(|e| e.tool_use_id.as_deref() == Some(tool.as_str()))
            .unwrap_or(0);
    s.open.insert(collapse_id(call));
    let mut rt = runtime(s);
    let row = d_key(&rt, call);
    scroll_to(&mut rt, &row);
    assert!(
        has_text(&rt, "Show full output (200.0 KB)"),
        "big output button"
    );
}

fn d_key(rt: &Runtime<Ui>, call: usize) -> String {
    let d = rt
        .state
        .detail
        .as_ref()
        .unwrap_or_else(|| panic!("no detail"));
    d.plan
        .iter()
        .position(|n| matches!(n, ChatNode::Tool { call: Some(c), .. } if *c == call))
        .map(|i| d.keys[i].clone())
        .unwrap_or_default()
}

/// Wheel the detail column until `key` is mounted.
fn scroll_to(rt: &mut Runtime<Ui>, key: &str) {
    for _ in 0..400 {
        if rt.find(key).is_some_and(|(_, y)| y > 0.0 && y < SIZE.1) {
            settle(rt);
            return;
        }
        rt.wheel(1000.0, 600.0, -600.0);
        settle(rt);
    }
    panic!("never reached {key}");
}

#[test]
fn stub_routes() {
    let mut s = state(&home::build_home());
    s.route = Route::Stats;
    let rt = runtime(s);
    assert!(has_text(&rt, "Stats is not in the desktop app yet."));
}

#[test]
fn watch_bumps_settle_into_one_refetch() {
    let mut s = state(&home::build_home());
    s.io = None;
    open(&mut s, LONG_ID);
    s.pending.clear();
    let mut tx = Tx::default();
    s.on_versions(
        WatchVersions {
            memory: 0,
            sessions: 0,
        },
        0.0,
        &mut tx,
    );
    for sec in 0..=3_u32 {
        s.on_versions(
            WatchVersions {
                memory: 0,
                sessions: u64::from(sec) + 1,
            },
            f64::from(sec),
            &mut tx,
        );
        s.tick(f64::from(sec) + 0.5, now_ms(), &mut tx);
    }
    assert_eq!(s.pending, vec![]);
    s.tick(4.0, now_ms(), &mut tx);
    assert_eq!(s.pending.len(), 2);
}
