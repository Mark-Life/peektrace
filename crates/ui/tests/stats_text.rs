//! The inspector's stats text rules on its own fixtures
//! (`apps/inspector/test/stats-format.test.tsx`, `session-chips.test.tsx`):
//! findings order and impact, rule lines, suggested rules, time notes,
//! project tails and session chips. Core writes the sentences in the
//! inspector's number format (`derive::text_of`); ui owns the chips.

mod common;

use common::stats_report::{
    PARENT, cause_row, fail_row, firing, main_sample, report, sample, sample_at, three_causes,
    time_row,
};
use peektrace_core::Agent;
use peektrace_core::stats::accumulate::SampleRef;
use peektrace_core::stats::schema::Job;
use peektrace_core::stats::service::StatsReport;
use peektrace_protocol::DrillTable;
use peektrace_protocol::stats::{SampleRefView, StatsReportView};
use peektrace_ui::model::stats::StatsText;
use peektrace_ui::model::stats::chips::{
    PROJECT_TAIL, SessionTitles, chip_label_of, project_tail, session_chips, title_key,
};
use peektrace_ui::model::stats::derive::text_of;

fn text(r: &StatsReport) -> StatsText {
    text_of(r, None)
}

/// Samples as the views read them: through the report JSON.
fn views(samples: Vec<SampleRef>) -> Vec<SampleRefView> {
    let r = report(vec![], vec![fail_row("x", |f| f.samples = samples)], vec![]);
    let v: StatsReportView = serde_json::from_str(&r.to_json(false)).expect("report view");
    v.fails
        .into_iter()
        .next()
        .map(|f| f.samples)
        .unwrap_or_default()
}

fn one(s: SampleRef) -> SampleRefView {
    views(vec![s]).remove(0)
}

fn titled(pairs: &[(&str, &str)]) -> SessionTitles {
    pairs
        .iter()
        .map(|(id, t)| {
            (
                title_key(peektrace_protocol::AgentId::Claude, id),
                (*t).to_owned(),
            )
        })
        .collect()
}

// stats-format.test.tsx:73
#[test]
fn a_time_row_says_only_what_its_own_columns_measured() {
    let r = report(
        vec![],
        vec![],
        vec![
            time_row(Job::Typecheck, |_| {}),
            time_row(Job::Test, |t| {
                t.bg_runs = 3.0;
                t.capped_runs = 2.0;
                t.capped_sec = 60.0;
            }),
            time_row(Job::WaitPoll, |t| t.bg_runs = 3.0),
        ],
    );
    let notes = text(&r).time_notes;
    assert_eq!(notes.get("typecheck"), None);
    assert!(notes["test"].contains("harness time limit"), "{notes:?}");
    assert!(notes["test"].contains("background"), "{notes:?}");
    assert!(notes["wait/poll"].contains("background"), "{notes:?}");
}

// stats-format.test.tsx:83
#[test]
fn fail_and_time_rows_are_never_findings_except_one_folded_piped_finding() {
    let piped = |key: &str, n: f64| {
        fail_row(key, |f| {
            f.detector = Some("piped-exit-code".into());
            f.note = Some("555 of 576 pipe into head/tail".into());
            f.piped = n;
            f.sessions = 225.0;
        })
    };
    let r = report(
        vec![],
        vec![
            piped("scan:tsc", 5.0),
            piped("scan:jest", 1.0),
            fail_row("detect:zsh-equals-abort", |f| {
                f.detector = Some("zsh-equals-abort".into());
                f.note = Some("quote the separator: echo \"===\"".into());
                f.sessions = 91.0;
            }),
            fail_row("flag:Bash", |f| f.sessions = 300.0),
        ],
        vec![time_row(Job::Test, |t| {
            t.capped_runs = 4.0;
            t.capped_sec = 300.0;
        })],
    );
    let findings = text(&r).findings;
    let keys: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["scan:tsc"]);
    assert_eq!(findings[0].title, "Piped output hides failed exit codes");
    assert_eq!(findings[0].impact, "6 calls");
}

// stats-format.test.tsx:115
#[test]
fn project_tail_keeps_short_slugs_and_cuts_long_ones_at_a_dash() {
    let tail = |p: &str| project_tail(p, PROJECT_TAIL);
    assert_eq!(tail("peephole"), "peephole");
    assert_eq!(
        tail("-Users-andrey-Library-Mobile-Documents-iCloud-md-obsidian-Documents-Obsidian-Vault"),
        "…Obsidian-Vault"
    );
    assert_eq!(
        tail("-Users-andrey-Code-personal-peephole"),
        "…personal-peephole"
    );
    assert_eq!(tail("-Users-x-personal-thinkrail"), "…personal-thinkrail");
    assert_eq!(
        project_tail(&"a".repeat(30), 10),
        format!("…{}", "a".repeat(10))
    );
}

const CHIP_PARENT: &str = "3f2a1b4c-9d8e-4f00-8000-000000000000";
const SUBAGENT: &str = "agent-aac481ccea1ab4b5a";

// stats-format.test.tsx:134
#[test]
fn a_subagent_chip_names_its_parent_session_and_says_subagent() {
    let sub = one(sample(|s| {
        s.project = "-Users-andrey-Code-jetbrains-thinkrail".into();
        s.sid = SUBAGENT.into();
        s.parent = Some(CHIP_PARENT.into());
    }));
    let chip = chip_label_of(&sub, &SessionTitles::new());
    assert_eq!(
        (chip.project.as_str(), chip.session.as_str(), chip.subagent),
        ("…thinkrail", "3f2a1b4c", true)
    );
    let main = one(sample(|s| {
        s.project = "p".into();
        s.sid = CHIP_PARENT.into();
        s.parent = None;
    }));
    let chip = chip_label_of(&main, &SessionTitles::new());
    assert_eq!(
        (chip.project.as_str(), chip.session.as_str(), chip.subagent),
        ("p", "3f2a1b4c", false)
    );
}

// stats-format.test.tsx:236
#[test]
fn a_fail_row_with_no_fix_is_not_a_finding() {
    let r = report(
        vec![cause_row(|_| {})],
        vec![fail_row("scan:tsc", |f| {
            f.note = Some("pipes".into());
            f.sessions = 225.0;
        })],
        vec![],
    );
    let findings = text(&r).findings;
    let order: Vec<(Option<DrillTable>, &str)> =
        findings.iter().map(|f| (f.table, f.key.as_str())).collect();
    assert_eq!(order, [(Some(DrillTable::Cause), "cause:zsh-glob-flag")]);
    assert_eq!(
        findings[0].impact,
        "675 retries (main 41 / sub 634) · 44m20s · 32.3M ctx tokens re-read"
    );
}

// stats-format.test.tsx:253
#[test]
fn a_rule_already_present_says_so_with_its_rate_since_the_edit() {
    let r = report(vec![cause_row(|c| c.rule = Some(firing()))], vec![], vec![]);
    assert_eq!(
        text(&r).rule_lines["cause:zsh-glob-flag"],
        "rule in ~/.claude/CLAUDE.md:100 · still firing main 31/1k, sub 57/1k since 2026-10-15"
    );
}

// stats-format.test.tsx:259 (the rendered part is in tests/stats_views.rs)
#[test]
fn suggested_rules_skip_causes_a_memory_file_already_states() {
    assert_eq!(
        text(&three_causes()).suggested,
        ["quote every glob passed as a flag value"]
    );
}

// stats-format.test.tsx:439
#[test]
fn session_titles_never_reach_the_copied_prompts() {
    let r = report(
        vec![],
        vec![fail_row("scan:tsc", |f| {
            f.samples = vec![sample(|s| s.sid = "sub-0".into())];
        })],
        vec![],
    );
    let prompt = text(&r).prompts.fail["scan:tsc"].clone();
    assert!(prompt.contains("scan:tsc"), "{prompt}");
    assert!(!prompt.contains("Fix the test"), "{prompt}");
}

// session-chips.test.tsx:23
#[test]
fn samples_under_one_parent_fold_into_one_chip_order_kept() {
    let samples = views(vec![
        sample_at("sub-0", None, 0.0),
        sample_at("sub-1", Some("bbbbbbbb-2222"), 1.0),
        sample_at("sub-2", None, 2.0),
        sample_at("sub-3", None, 3.0),
    ]);
    let groups = session_chips(&samples);
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [3, 1]);
    assert_eq!(groups[0][0].sid, "sub-0");
    assert_eq!(groups[1][0].parent.as_deref(), Some("bbbbbbbb-2222"));
}

// session-chips.test.tsx:35
#[test]
fn a_main_sample_and_a_subagent_sample_of_one_session_share_a_chip() {
    let samples = views(vec![main_sample(PARENT), sample_at("sub-1", None, 1.0)]);
    assert_eq!(session_chips(&samples).len(), 1);
}

// session-chips.test.tsx:43
#[test]
fn the_same_id_under_two_agents_stays_two_chips() {
    let samples = views(vec![
        main_sample("same-id"),
        SampleRef {
            agent: Agent::Codex,
            ..main_sample("same-id")
        },
    ]);
    assert_eq!(session_chips(&samples).len(), 2);
}

// session-chips.test.tsx:51
#[test]
fn a_known_title_labels_the_chip_else_project_id() {
    let s = one(sample(|_| {}));
    let titles = titled(&[(PARENT, "  Fix the\n flaky   test ")]);
    assert_eq!(chip_label_of(&s, &titles).label, "Fix the flaky test");
    assert_eq!(
        chip_label_of(&s, &SessionTitles::new()).label,
        "demo-app/aaaaaaaa"
    );
    assert_eq!(
        chip_label_of(&s, &titled(&[(PARENT, "  ")])).label,
        "demo-app/aaaaaaaa"
    );
}

// session-chips.test.tsx:64
#[test]
fn a_long_title_is_cut_with_an_ellipsis() {
    let label = chip_label_of(&one(sample(|_| {})), &titled(&[(PARENT, &"x".repeat(90))])).label;
    assert!(label.encode_utf16().count() < 50, "{label}");
    assert!(label.ends_with('…'), "{label}");
}

// session-chips.test.tsx:73
#[test]
fn a_long_title_is_not_cut_inside_a_surrogate_pair() {
    let title = format!("{}\u{1F600}tail more text", "a".repeat(38));
    let label = chip_label_of(&one(sample(|_| {})), &titled(&[(PARENT, &title)])).label;
    assert_eq!(label, format!("{}\u{1F600}…", "a".repeat(38)));
}

// session-chips.test.tsx:104 (the rendered part is in tests/stats_views.rs)
#[test]
fn a_title_keeps_the_short_id_beside_it_so_equal_titles_differ() {
    let titles = titled(&[(PARENT, "Fix tests"), ("bbbbbbbb-2222", "Fix tests")]);
    let a = chip_label_of(&one(sample(|_| {})), &titles);
    let b = chip_label_of(
        &one(sample(|s| s.parent = Some("bbbbbbbb-2222".into()))),
        &titles,
    );
    assert_eq!(a.label, b.label);
    assert_eq!(a.id.as_deref(), Some("aaaaaaaa"));
    assert_eq!(b.id.as_deref(), Some("bbbbbbbb"));
    assert_eq!(
        chip_label_of(&one(sample(|_| {})), &SessionTitles::new()).id,
        None
    );
}
