//! The Stats screen driven through Rune's `Runtime` on a synthetic HOME,
//! with core answering in process. A failing source stands in for errors.

#![allow(
    clippy::assert_is_empty,
    reason = "the keys print on failure where it matters"
)]

mod common;

use std::sync::Arc;

use common::home::{self, STATS_MAIN_ID};
use peektrace_protocol::stats::DrillView;
use peektrace_protocol::{AgentId, ApiError, DrillTable};
use peektrace_ui::model::stats::{DrillAsk, StatsScope};
use peektrace_ui::source::in_process::InProcess;
use peektrace_ui::source::{DataSource, LoadError, OnVersions, Reply, StatsScreen};
use peektrace_ui::state::{Io, Request, Route, Ui};
use peektrace_ui::state_stats::CopyState;
use peektrace_ui::{load_now, views};
use rune::{App, Env, Key, KeyEvent, Mods, Runtime};

fn now_ms() -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
    peektrace_js::time::parse_date(home::NOW, &peektrace_js::time::Utc).map_or(0.0, |ms| ms as f64)
}

fn io(source: Arc<dyn DataSource>) -> Io {
    Io {
        source,
        handle: App::new(peektrace_ui::spec()).handle(),
    }
}

fn source(home: &std::path::Path) -> Arc<dyn DataSource> {
    let dir = home.to_string_lossy().into_owned();
    let peektrace = format!("{dir}/.peektrace");
    Arc::new(
        InProcess::for_home(dir, move |k| {
            (k == "PEEKTRACE_DIR").then(|| peektrace.clone())
        })
        .at(now_ms()),
    )
}

/// The Stats route on the stats HOME, report loaded.
fn stats_state() -> Ui {
    let mut s = Ui::new(Some(io(source(&home::stats_home()))), now_ms(), true);
    s.request_sessions();
    s.enter_stats(0.0);
    load_now(&mut s);
    s
}

fn runtime_at(s: Ui, size: (f32, f32)) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, size, Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn runtime(s: Ui) -> Runtime<Ui> {
    runtime_at(s, (1440.0, 900.0))
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

fn keys_with(rt: &Runtime<Ui>, prefix: &str) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with(prefix))
        .collect()
}

fn rect(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .map_or_else(|| panic!("no element {key}"), |e| e.rect)
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Scroll the stats column until `key` is on screen, then press it.
fn press(rt: &mut Runtime<Ui>, key: &str) {
    for _ in 0..40 {
        if let Some((x, y)) = rt.find(key)
            && (0.0..880.0).contains(&y)
        {
            rt.pointer_move(x, y);
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            settle(rt);
            return;
        }
        rt.wheel(900.0, 500.0, -400.0);
        settle(rt);
    }
    panic!("never reached {key}");
}

fn load(rt: &mut Runtime<Ui>) {
    rt.update(|s, _| load_now(s));
    settle(rt);
}

#[test]
fn report_header_tables_and_findings() {
    let rt = runtime(stats_state());
    assert!(has_text(
        &rt,
        "2 sessions + 1 subagent runs · 1 projects · 23 tool calls · 23 shell calls · 12m40s of shell time"
    ));
    assert!(has_text(
        &rt,
        "2026-04-02 → 2026-06-01 · generated 6/1/2026, "
    ));
    assert!(has_text(&rt, "What fails, what costs time"));
    let cards = keys_with(&rt, "stats-finding:");
    assert_eq!(
        cards.first().map(String::as_str),
        Some("stats-finding:cause:cause:zsh-glob-path")
    );
    assert!(has_text(
        &rt,
        "3 retries (main 2 / sub 1) · 21s · 6.6K ctx tokens re-read"
    ));
    assert!(!keys_with(&rt, "cause-row:").is_empty());
    assert_eq!(rt.state.title(), "Peektrace");
}

#[test]
fn an_open_cause_row_shows_its_split_rule_and_prompt() {
    let mut rt = runtime(stats_state());
    assert!(
        keys_with(&rt, "cause-detail:").is_empty(),
        "{:?}",
        keys_with(&rt, "cause-detail")
    );
    press(&mut rt, "cause-row:cause:command-not-found");
    assert_eq!(
        keys_with(&rt, "cause-detail:"),
        ["cause-detail:cause:command-not-found"]
    );
    assert!(has_text(
        &rt,
        "hits main 2 / sub 1 · per 1k calls in this window main"
    ));
    assert!(has_text(&rt, "rule in ~/.claude/CLAUDE.md:3"));
    assert!(
        rt.find("cause-detail:cause:command-not-found/cause-prompt")
            .is_some()
    );
    assert!(
        rt.find("cause-row:cause:zsh-glob-path/cause-prompt")
            .is_none()
    );
    let prompt = rt
        .state
        .stats
        .screen()
        .and_then(|x| x.text.as_ref())
        .and_then(|t| t.prompts.cause.get("cause:command-not-found").cloned())
        .unwrap_or_default();
    assert!(prompt.starts_with("Goal: make my coding agents faster"));
    assert!(
        !prompt.contains("Run the checks"),
        "no session title in a prompt"
    );
    press(&mut rt, "cause-row:cause:command-not-found");
    assert!(keys_with(&rt, "cause-detail:").is_empty());
}

#[test]
fn a_narrow_column_folds_the_cause_columns_into_one_line() {
    let rt = runtime_at(stats_state(), (700.0, 900.0));
    assert!(!keys_with(&rt, "cause-narrow:").is_empty());
    assert!(!texts(&rt).iter().any(|t| t == "Hits"));
    let wide = runtime(stats_state());
    assert!(keys_with(&wide, "cause-narrow:").is_empty());
    assert!(texts(&wide).iter().any(|t| t == "Hits"));
}

#[test]
fn a_finding_impact_stays_inside_its_card() {
    for w in [1440.0, 900.0] {
        let rt = runtime_at(stats_state(), (w, 900.0));
        let card = rect(&rt, "stats-finding:cause:cause:zsh-glob-path");
        let (x, y) = rt
            .find("stats-finding:cause:cause:zsh-glob-path/stats-finding-impact")
            .expect("impact");
        assert!(
            x > card[0] && x < card[0] + card[2],
            "{w}: {x} outside {card:?}"
        );
        assert!(
            y > card[1] && y < card[1] + card[3],
            "{w}: {y} outside {card:?}"
        );
    }
}

#[test]
fn a_subagent_chip_opens_through_its_parent_at_the_call() {
    let mut rt = runtime(stats_state());
    let chip = keys_with(&rt, "stats-session-link:agent-a1")
        .into_iter()
        .next()
        .expect("subagent chip");
    click(&mut rt, &chip);
    assert_eq!(rt.state.route, Route::Sessions);
    let sel = rt.state.selected.clone().expect("selected");
    assert_eq!(sel.id, "agent-a1");
    assert_eq!(sel.parent.as_deref(), Some(STATS_MAIN_ID));
    assert!(
        rt.state
            .pending_call
            .as_deref()
            .is_some_and(|c| c.starts_with("toolu_agent-a1_"))
    );
    load(&mut rt);
    assert_eq!(
        rt.state.pending_call, None,
        "the call opened once the analysis landed"
    );
    assert!(rt.state.open.iter().any(|id| id.starts_with("chat:")));
}

#[test]
fn a_chip_into_the_open_session_opens_the_call_at_once() {
    let mut rt = runtime(stats_state());
    let chip = keys_with(&rt, "stats-session-link:agent-a1")
        .into_iter()
        .next()
        .expect("subagent chip");
    click(&mut rt, &chip);
    load(&mut rt);
    rt.update(|s, _| {
        s.enter_stats(0.0);
        s.open.clear();
    });
    settle(&mut rt);
    click(&mut rt, &chip);
    assert_eq!(rt.state.route, Route::Sessions);
    assert!(
        rt.state.pending.is_empty(),
        "no reload for the same session"
    );
    assert_eq!(rt.state.pending_call, None);
    assert!(rt.state.open.iter().any(|id| id.starts_with("chat:")));
}

#[test]
fn a_pending_call_drops_when_another_session_opens() {
    let mut rt = runtime(stats_state());
    let chip = keys_with(&rt, "stats-session-link:agent-a1")
        .into_iter()
        .next()
        .expect("subagent chip");
    click(&mut rt, &chip);
    assert!(rt.state.pending_call.is_some());
    rt.update(|s, _| s.select(STATS_MAIN_ID, None));
    assert_eq!(rt.state.pending_call, None);
}

#[test]
fn folded_samples_list_every_target() {
    let mut rt = runtime(stats_state());
    let more = keys_with(&rt, "stats-session-more:")
        .into_iter()
        .next()
        .expect("a folded chip");
    click(&mut rt, &more);
    let targets = keys_with(&rt, "stats-session-target:");
    assert!(targets.len() >= 2, "{targets:?}");
    assert!(has_text(&rt, "main · "));
}

#[test]
fn the_drill_sheet_pages_and_closes() {
    let mut rt = runtime(stats_state());
    press(&mut rt, "cause-row:cause:zsh-glob-path");
    press(&mut rt, "cause-drill");
    assert!(
        rt.state
            .pending
            .iter()
            .any(|r| matches!(r, Request::StatsDrill { .. }))
    );
    load(&mut rt);
    assert!(rt.find("stats-drill").is_some());
    assert!(has_text(&rt, "1–3 of 3 calls"));
    assert!(rt.find("drill-more").is_none() && rt.find("drill-back").is_none());
    assert!(!keys_with(&rt, "drill-hit:").is_empty());
    assert!(!keys_with(&rt, "drill-retry:").is_empty());
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(rt.state.stats.drill.is_none());
    assert!(rt.find("stats-drill").is_none());
}

/// The `zsh-glob-path` cause drill, as the UI caches it.
fn glob_drill(rt: &Runtime<Ui>) -> Vec<DrillAsk> {
    rt.state
        .pending
        .iter()
        .filter_map(|r| match r {
            Request::StatsDrill { ask, .. } if ask.key == "cause:zsh-glob-path" => {
                Some(ask.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn rescan_drops_the_cached_drill_pages() {
    let mut rt = runtime(stats_state());
    press(&mut rt, "cause-row:cause:zsh-glob-path");
    press(&mut rt, "cause-drill");
    load(&mut rt);
    rt.update(|s, tx| {
        s.close_drill();
        s.stats_ask(tx.clock());
    });
    load(&mut rt);
    assert!(
        rt.state.stats.drills.is_empty(),
        "{:?}",
        rt.state.stats.drills.keys()
    );
    rt.update(|s, _| s.open_drill(DrillTable::Cause, "cause:zsh-glob-path"));
    assert_eq!(glob_drill(&rt).len(), 1, "the drill asks again");
}

#[test]
fn rescan_asks_the_open_drill_again_from_its_first_page() {
    let mut rt = runtime(stats_state());
    press(&mut rt, "cause-row:cause:zsh-glob-path");
    press(&mut rt, "cause-drill");
    load(&mut rt);
    rt.update(|s, tx| {
        s.stats.page = ("cause:cause:zsh-glob-path".to_owned(), 25);
        s.stats_ask(tx.clock());
        load_now(s);
    });
    let asks = glob_drill(&rt);
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].offset, 0);
}

#[test]
fn a_fail_row_opens_its_drill_and_its_detail() {
    let mut rt = runtime(stats_state());
    press(&mut rt, "fail-row:scan:test");
    load(&mut rt);
    assert!(rt.find("fail-detail:scan:test").is_some());
    assert!(has_text(&rt, "of 1 calls") || has_text(&rt, "calls"));
}

#[test]
fn a_scope_change_reuses_the_cached_report() {
    let mut s = stats_state();
    s.set_stats_scope(30, None, 1.0);
    assert!(matches!(
        s.pending.as_slice(),
        [Request::StatsReport { .. }]
    ));
    load_now(&mut s);
    s.set_stats_scope(60, None, 2.0);
    assert!(s.pending.is_empty(), "60 days is cached");
    s.set_stats_scope(60, Some(AgentId::Codex), 3.0);
    assert_eq!(s.pending.len(), 1);
}

#[test]
fn rescan_in_flight_disables_and_says_so() {
    let mut s = stats_state();
    s.stats_ask(5.0);
    let mut rt = runtime(s);
    assert!(has_text(&rt, "Rescanning…"));
    assert!(rt.find("stats-corpus").is_some(), "the old report stays");
    load(&mut rt);
    assert!(has_text(&rt, "Rescan") && !has_text(&rt, "Rescanning…"));
}

#[test]
fn pending_counts_from_the_first_ask_not_from_mount() {
    let mut s = Ui::new(Some(io(source(&home::stats_home()))), now_ms(), true);
    s.enter_stats(10.0);
    let mut rt = runtime(s);
    assert!(rt.find("stats-pending").is_some());
    assert!(has_text(
        &rt,
        "Reading transcripts. The first pass parses every file"
    ));
    rt.update(|s, tx| {
        s.stats_tick(13.4, tx);
    });
    settle(&mut rt);
    assert!(texts(&rt).iter().any(|t| t == "3 s"));
    rt.update(|s, _| {
        s.leave_stats();
        s.route = Route::Sessions;
        s.enter_stats(20.0);
    });
    rt.update(|s, tx| {
        s.stats_tick(25.0, tx);
    });
    settle(&mut rt);
    assert!(texts(&rt).iter().any(|t| t == "15 s"), "{:?}", texts(&rt));
}

/// A runtime whose copies go to an in-memory clipboard.
fn with_clipboard(s: Ui) -> (Runtime<Ui>, rune::clipboard::MemoryClipboard) {
    let mut rt = runtime(s);
    let board = rune::clipboard::memory();
    rt.use_clipboard(&board);
    (rt, board)
}

#[test]
fn copy_says_copied_then_reverts() {
    let (mut rt, board) = with_clipboard(stats_state());
    rt.update(|s, tx| s.copy_text("stats-prompt".into(), "x".into(), tx));
    settle(&mut rt);
    assert_eq!(board.text().as_deref().map(String::as_str), Some("x"));
    assert!(has_text(&rt, "Copied"));
    let until = match rt.state.stats.copy {
        Some((_, CopyState::Copied { until })) => until,
        ref other => panic!("not copied: {other:?}"),
    };
    rt.update(|s, tx| {
        s.stats_tick(until + 0.1, tx);
    });
    settle(&mut rt);
    assert!(!has_text(&rt, "Copied"));
}

#[test]
fn a_failed_copy_says_so_and_shows_the_text() {
    let (mut rt, board) = with_clipboard(stats_state());
    board.fail_writes(Some("no clipboard"));
    press(&mut rt, "stats-prompt");
    assert_eq!(
        rt.state.stats.copy,
        Some(("stats-prompt".to_owned(), CopyState::Failed))
    );
    assert!(has_text(&rt, "Copy failed") && !has_text(&rt, "Copied"));
    assert!(rt.find("stats-prompt-manual").is_some());
    board.fail_writes(None);
    press(&mut rt, "stats-prompt");
    assert!(has_text(&rt, "Copied") && rt.find("stats-prompt-manual").is_none());
}

#[test]
fn the_watch_never_reloads_the_report() {
    let mut s = stats_state();
    s.pending.clear();
    let mut tx = rune::Tx::default();
    for n in 0..3 {
        s.on_versions(
            peektrace_protocol::WatchVersions {
                memory: 0,
                sessions: n,
            },
            f64::from(u32::try_from(n).unwrap_or(0)),
            &mut tx,
        );
    }
    s.tick(10.0, now_ms(), &mut tx);
    assert!(
        !s.pending
            .iter()
            .any(|r| matches!(r, Request::StatsReport { .. }))
    );
}

#[test]
fn an_empty_window_says_so_everywhere() {
    let mut s = Ui::new(
        Some(io(source(&home::temp_home("stats-empty")))),
        now_ms(),
        true,
    );
    s.enter_stats(0.0);
    load_now(&mut s);
    let rt = runtime(s);
    for key in [
        "stats-no-findings",
        "cause-empty",
        "fail-empty",
        "time-empty",
    ] {
        assert!(rt.find(key).is_some(), "{key}");
    }
    assert!(rt.find("stats-clusters/rank-empty").is_some());
}

#[test]
fn the_nav_clears_the_traffic_lights() {
    let mut s = stats_state();
    s.nav_collapsed = true;
    let mut rt = runtime(s);
    assert!((rect(&rt, "nav")[2] - 56.0).abs() < 0.5);
    assert!(rt.find("titlebar-band").is_none());
    rt.set_titlebar_inset(67.0);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    assert!((rect(&rt, "nav")[2] - 80.0).abs() < 0.5);
    assert!(rt.find("titlebar-band").is_some());
}

/// A source whose stats calls fail.
struct Failing;

fn scan_failed<T>() -> Result<T, LoadError> {
    Err(LoadError::Api(ApiError::StatsScanError {
        reason: "disk on fire".into(),
        path: None,
    }))
}

impl DataSource for Failing {
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
    fn stats_report(&self, _: StatsScope, reply: Reply<StatsScreen>) {
        reply(scan_failed());
    }
    fn stats_drill(&self, _: StatsScope, _: DrillAsk, reply: Reply<DrillView>) {
        reply(scan_failed());
    }
    fn stats_report_now(&self, _: StatsScope) -> Option<Result<StatsScreen, LoadError>> {
        Some(scan_failed())
    }
}

#[test]
fn a_failed_pass_shows_the_error_and_retries() {
    let mut s = Ui::new(Some(io(Arc::new(Failing))), now_ms(), true);
    s.enter_stats(0.0);
    load_now(&mut s);
    let mut rt = runtime(s);
    assert!(rt.find("result-error").is_some());
    assert!(has_text(&rt, "Something went wrong"));
    assert!(has_text(&rt, "disk on fire"));
    click(&mut rt, "result-retry");
    assert!(has_text(&rt, "Retrying…"));
}

#[test]
fn a_failed_rescan_over_a_report_shows_the_error() {
    let mut s = stats_state();
    s.stats_ask(1.0);
    let rid = s.stats.rid;
    s.pending.clear();
    s.apply_stats(rid, s.stats.scope, scan_failed());
    let rt = runtime(s);
    assert!(rt.find("result-error").is_some());
    assert!(rt.find("stats-corpus").is_none());
}
