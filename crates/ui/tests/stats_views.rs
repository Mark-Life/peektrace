//! The inspector's rendered stats tests on their own fixtures
//! (`stats-format.test.tsx`, `session-chips.test.tsx`, `report-header.test.tsx`,
//! `result-view.test.tsx`, `stats-pending.test.tsx`), through Rune's
//! `Runtime`. The report lands in the state directly, as a source would.

#![allow(
    clippy::assert_is_empty,
    reason = "the keys print on failure where it matters"
)]

mod common;

use common::stats_report::{
    PARENT, cause_row, fail_row, firing, main_sample, report, sample, sample_at, three_causes,
    time_row,
};
use peektrace_core::stats::accumulate::SampleRef;
use peektrace_core::stats::schema::Job;
use peektrace_core::stats::service::StatsReport;
use peektrace_protocol::view::SessionListingView;
use peektrace_protocol::{ApiError, DrillTable};
use peektrace_ui::model::stats::StatsScope;
use peektrace_ui::model::stats::derive::text_of;
use peektrace_ui::model::stats::format::at;
use peektrace_ui::source::{LoadError, StatsScreen, decode};
use peektrace_ui::state::{Route, Ui};
use peektrace_ui::views;
use rune::{ElementInfo, Env, Runtime, Tx};

fn screen(r: &StatsReport) -> StatsScreen {
    StatsScreen {
        report: decode(&r.to_json(false)).expect("report view"),
        text: Some(text_of(r, None)),
        generated: at(r.generated_at, 0.0),
    }
}

/// The Stats route at 30 days with `result` landed for it.
fn landed(result: Result<StatsScreen, LoadError>) -> Ui {
    let mut s = Ui::new(None, 0.0, true);
    s.stats.scope = StatsScope {
        agent: None,
        days: 30,
    };
    s.enter_stats(0.0);
    let rid = s.stats.rid;
    s.apply_stats(rid, s.stats.scope, result);
    s.pending.clear();
    s
}

fn ui(r: &StatsReport) -> Ui {
    landed(Ok(screen(r)))
}

/// Session titles in the rail listing, as `sessions` would land them.
fn with_titles(mut s: Ui, titles: &[(&str, &str)]) -> Ui {
    let headers: Vec<serde_json::Value> = titles
        .iter()
        .map(|(id, title)| {
            serde_json::json!({
                "id": id, "agent": "claude", "path": format!("/x/{id}.jsonl"),
                "project": "demo-app", "messageCount": 1, "sizeBytes": 1, "title": title,
            })
        })
        .collect();
    let listing: SessionListingView =
        serde_json::from_value(serde_json::json!({ "headers": headers, "unreadable": [] }))
            .expect("listing");
    s.rail.listing = Some(listing);
    s
}

fn runtime_at(s: Ui, w: f32) -> Runtime<Ui> {
    let mut rt = Runtime::new(s, views::view, (w, 900.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn runtime(s: Ui) -> Runtime<Ui> {
    runtime_at(s, 1440.0)
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

fn keyed(rt: &Runtime<Ui>, key: &str) -> Vec<ElementInfo> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.key.as_deref() == Some(key))
        .collect()
}

fn el(rt: &Runtime<Ui>, key: &str) -> ElementInfo {
    keyed(rt, key)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no element {key}"))
}

fn keys_with(rt: &Runtime<Ui>, prefix: &str) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with(prefix))
        .collect()
}

fn click(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn open_cause(mut s: Ui, key: &str) -> Ui {
    s.toggle_cause(key);
    s
}

/// A cause with no fix, so no finding repeats its links: only the open
/// detail row shows `samples`.
fn links_of(samples: Vec<SampleRef>, titles: &[(&str, &str)]) -> Runtime<Ui> {
    let r = report(
        vec![cause_row(|c| {
            c.fix = None;
            c.samples = samples;
        })],
        vec![],
        vec![],
    );
    runtime(with_titles(
        open_cause(ui(&r), "cause:zsh-glob-flag"),
        titles,
    ))
}

// report-header.test.tsx:10
#[test]
fn a_rescan_in_flight_disables_the_button_and_says_so() {
    let r = report(vec![], vec![], vec![]);
    let idle = runtime(ui(&r));
    assert!(has_text(&idle, "Rescan") && !has_text(&idle, "Rescanning…"));
    assert!(el(&idle, "stats-rescan").focusable, "enabled at rest");
    let mut s = ui(&r);
    s.stats_ask(1.0);
    let busy = runtime(s);
    assert!(has_text(&busy, "Rescanning…"));
    assert!(!el(&busy, "stats-rescan").focusable, "disabled in flight");
}

fn failed() -> Ui {
    landed(Err(LoadError::Api(ApiError::StatsScanError {
        reason: "boom".into(),
        path: None,
    })))
}

// result-view.test.tsx:26
#[test]
fn a_failure_shows_an_enabled_retry_button() {
    let rt = runtime(failed());
    assert!(rt.find("result-error").is_some());
    assert!(el(&rt, "result-retry").focusable);
    assert!(has_text(&rt, "Retry") && !has_text(&rt, "Retrying…"));
}

// result-view.test.tsx:40
#[test]
fn a_failure_being_refetched_disables_the_button() {
    let mut s = failed();
    s.stats_ask(1.0);
    let rt = runtime(s);
    assert!(!el(&rt, "result-retry").focusable);
    assert!(has_text(&rt, "Retrying…"));
}

// result-view.test.tsx:46
#[test]
fn a_success_renders_the_value_and_no_retry() {
    let rt = runtime(ui(&report(vec![], vec![], vec![])));
    assert!(rt.find("stats-corpus").is_some());
    assert!(rt.find("result-error").is_none() && rt.find("result-retry").is_none());
}

/// Leaves under `key`'s element with no text: the skeleton blocks.
fn blank_leaves_under(rt: &Runtime<Ui>, key: &str) -> usize {
    let els = rt.elements();
    let Some(root) = els.iter().position(|e| e.key.as_deref() == Some(key)) else {
        return 0;
    };
    let under = |mut i: usize| {
        while let Some(p) = els[i].parent {
            if p == root {
                return true;
            }
            i = p;
        }
        false
    };
    (0..els.len())
        .filter(|&i| under(i))
        .filter(|&i| !els.iter().any(|e| e.parent == Some(i)))
        .filter(|&i| els[i].text.is_none())
        .count()
}

// stats-pending.test.tsx:11
#[test]
fn pending_stats_show_a_status_note_a_counter_and_skeletons_but_no_progress_bar() {
    let mut s = Ui::new(None, 0.0, true);
    s.enter_stats(0.0);
    let rt = runtime(s);
    assert!(rt.find("stats-pending").is_some());
    assert_eq!(el(&rt, "stats-elapsed").text.as_deref(), Some("0 s"));
    assert!(rt.elements().iter().any(|e| e.role == "status"));
    assert!(!rt.elements().iter().any(|e| e.role == "progressbar"));
    let blocks = blank_leaves_under(&rt, "stats-pending");
    assert!(blocks > 5, "{blocks} skeleton blocks");
}

// stats-format.test.tsx:259
#[test]
fn suggested_rules_render_with_their_copy_buttons() {
    let rt = runtime(ui(&three_causes()));
    for key in ["stats-copy-all", "finding-copy", "finding-prompt"] {
        assert!(!keyed(&rt, key).is_empty(), "{key}");
    }
    assert!(texts(&rt).iter().any(|t| t == "retries"));
}

// stats-format.test.tsx:298
#[test]
fn the_cause_table_renders_its_rows_and_the_open_one_shows_the_rule_check() {
    let r = report(vec![cause_row(|c| c.rule = Some(firing()))], vec![], vec![]);
    let rt = runtime(open_cause(ui(&r), "cause:zsh-glob-flag"));
    assert!(rt.find("cause-row:cause:zsh-glob-flag").is_some());
    assert!(rt.find("cause-detail:cause:zsh-glob-flag").is_some());
    assert!(rt.find("cause-split:cause:zsh-glob-flag").is_some());
    for needle in [
        "still firing main 31/1k, sub 57/1k",
        "setopt nonomatch",
        "hits main 125 / sub 1,363",
        "context re-read 32.3M",
        "--include=*.ts → --include='*.ts' (135)",
        "per 1k calls in this window main 26 / sub 53",
    ] {
        assert!(has_text(&rt, needle), "{needle}: {:?}", texts(&rt));
    }
    assert!(texts(&rt).iter().any(|t| t == "Hits main / sub"));
}

// stats-format.test.tsx:321
#[test]
fn a_phone_reads_each_cause_row_as_one_wrapped_summary_line() {
    let r = report(vec![cause_row(|c| c.rule = Some(firing()))], vec![], vec![]);
    let rt = runtime_at(ui(&r), 700.0);
    assert!(rt.find("cause-narrow:cause:zsh-glob-flag").is_some());
    assert!(has_text(
        &rt,
        "1,488 hits · 675 retries in 550 sessions · 44m20s · 32.3M ctx re-read"
    ));
}

// stats-format.test.tsx:339
#[test]
fn a_findings_impact_wraps_instead_of_running_past_the_card() {
    let r = report(vec![cause_row(|_| {})], vec![], vec![]);
    let card = "stats-finding:cause:cause:zsh-glob-flag";
    for w in [1440.0, 900.0, 700.0] {
        let rt = runtime_at(ui(&r), w);
        let [cx, cy, cw, ch] = el(&rt, card).rect;
        let [x, y, iw, ih] = el(&rt, "stats-finding-impact").rect;
        assert!(
            x >= cx && x + iw <= cx + cw + 0.5 && y >= cy && y + ih <= cy + ch + 0.5,
            "{w}: impact {:?} outside card {:?}",
            [x, y, iw, ih],
            [cx, cy, cw, ch]
        );
    }
}

fn prompt_help(rt: &Runtime<Ui>, key: &str) -> String {
    el(rt, key).help.unwrap_or_default()
}

// stats-format.test.tsx:360
#[test]
fn every_open_row_and_the_header_copy_an_agent_prompt_each_named_for_its_row() {
    let r = report(
        vec![cause_row(|c| c.rule = Some(firing()))],
        vec![fail_row("scan:tsc", |f| {
            f.label = "error TS####".into();
            f.note = Some("pipes".into());
        })],
        vec![time_row(Job::Test, |t| t.label = "tests")],
    );
    let base = || open_cause(ui(&r), "cause:zsh-glob-flag");
    let rt = runtime(base());
    let mut helps = vec![
        prompt_help(&rt, "stats-prompt"),
        prompt_help(&rt, "cause-prompt"),
    ];
    for (table, key, button) in [
        (DrillTable::Fail, "scan:tsc", "fail-prompt"),
        (DrillTable::Time, "test", "time-prompt"),
    ] {
        let mut s = base();
        s.open_drill(table, key);
        helps.push(prompt_help(&runtime(s), button));
    }
    assert_eq!(
        helps,
        [
            "Copy prompt: investigate this report",
            "Copy prompt: investigate unquoted glob in a flag value",
            "Copy prompt: investigate error TS####",
            "Copy prompt: investigate tests time",
        ]
    );
    assert!(!helps.iter().any(|h| h.starts_with("Goal:")));
    assert!(texts(&rt).iter().any(|t| t == "Copy prompt"));
}

// stats-format.test.tsx:427
#[test]
fn a_closed_row_renders_no_prompt_button() {
    let r = report(vec![], vec![fail_row("scan:tsc", |_| {})], vec![]);
    let s = ui(&r);
    assert!(
        s.stats
            .screen()
            .and_then(|x| x.text.as_ref())
            .is_some_and(|t| t.prompts.fail.contains_key("scan:tsc")),
        "the row has a prompt to show"
    );
    let rt = runtime(s);
    assert!(rt.find("fail-row:scan:tsc").is_some());
    assert!(keyed(&rt, "fail-prompt").is_empty());
}

const CHIP_PARENT: &str = "3f2a1b4c-9d8e-4f00-8000-000000000000";
const SUBAGENT: &str = "agent-aac481ccea1ab4b5a";

// stats-format.test.tsx:152
#[test]
fn a_subagent_link_opens_the_subagent_through_its_parent() {
    let mut rt = links_of(
        vec![sample(|s| {
            s.sid = SUBAGENT.into();
            s.parent = Some(CHIP_PARENT.into());
        })],
        &[],
    );
    let chip = keys_with(&rt, "stats-session-link:")
        .into_iter()
        .next()
        .expect("chip");
    click(&mut rt, &chip);
    assert_eq!(rt.state.route, Route::Sessions);
    let sel = rt.state.selected.clone().expect("selected");
    assert_eq!(
        (sel.id.as_str(), sel.parent.as_deref()),
        (SUBAGENT, Some(CHIP_PARENT))
    );
    let mut s = Ui::new(None, 0.0, true);
    s.open_sample(CHIP_PARENT, None, None, &mut Tx::default());
    assert_eq!(s.selected.and_then(|x| x.parent), None);
}

// stats-format.test.tsx:171
#[test]
fn a_drill_link_names_the_call_to_scroll_to() {
    let mut s = Ui::new(None, 0.0, true);
    s.open_sample(
        CHIP_PARENT,
        None,
        Some("toolu_42".into()),
        &mut Tx::default(),
    );
    assert_eq!(s.pending_call.as_deref(), Some("toolu_42"));
    s.open_sample(CHIP_PARENT, None, None, &mut Tx::default());
    assert_eq!(s.pending_call, None);
}

fn three_under_parent() -> Vec<SampleRef> {
    (0..3)
        .map(|i| sample_at(&format!("sub-{i}"), None, f64::from(i)))
        .collect()
}

// session-chips.test.tsx:84
#[test]
fn same_parent_samples_show_one_button_and_a_count() {
    let rt = links_of(three_under_parent(), &[(PARENT, "Fix the test")]);
    assert_eq!(keys_with(&rt, "stats-session-link:").len(), 1);
    assert!(has_text(&rt, "Fix the test"));
    assert!(has_text(&rt, "×3"));
}

// session-chips.test.tsx:104
#[test]
fn a_title_keeps_the_short_id_beside_it_so_equal_titles_differ() {
    let rt = links_of(
        vec![
            sample(|_| {}),
            sample(|s| {
                s.parent = Some("bbbbbbbb-2222".into());
                s.seq = 1.0;
            }),
        ],
        &[(PARENT, "Fix tests"), ("bbbbbbbb-2222", "Fix tests")],
    );
    assert!(has_text(&rt, "aaaaaaaa") && has_text(&rt, "bbbbbbbb"));
}

// session-chips.test.tsx:126
#[test]
fn a_folded_chip_offers_one_target_per_sample() {
    let mut rt = links_of(three_under_parent(), &[]);
    let more = keys_with(&rt, "stats-session-more:")
        .into_iter()
        .next()
        .expect("a folded chip");
    click(&mut rt, &more);
    assert_eq!(keys_with(&rt, "stats-session-target:").len(), 3);
    for sid in ["sub-0", "sub-1", "sub-2"] {
        assert!(has_text(&rt, sid), "{sid}");
    }
}

// session-chips.test.tsx:141
#[test]
fn a_mixed_group_shows_the_subagent_tag() {
    let rt = links_of(
        vec![main_sample(PARENT), sample_at("sub-1", None, 1.0)],
        &[],
    );
    assert!(has_text(&rt, "subagent"));
    assert!(!keys_with(&rt, "stats-session-more:").is_empty());
}
