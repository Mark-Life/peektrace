//! The cold-start list: tier order, the sentence each row takes, and a share
//! of nothing that says so. TS injects stand-in number formatters; Rust has
//! one set, so the expected strings use the real ones (`50.0s`, `900`).
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

use peektrace_core::stats::accumulate::{CorpusSummary, DrillTable, StatsWindow};
use peektrace_core::stats::detectors::causes::CauseKind;
use peektrace_core::stats::detectors::corpus::CorpusPanels;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::rules::{RuleCheck, RuleMatch, Split};
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::detectors::waiting::WaitPanel;
use peektrace_core::stats::findings::{
    NO_RULE, cause_note, findings_of, no_suggestion_reason, rule_line, time_note, time_spread,
    working_of,
};
use peektrace_core::stats::format::rate_text;
use peektrace_core::stats::schema::{CauseId, Job};
use peektrace_core::stats::service::StatsReport;

fn split(main: f64, nested: f64) -> Split {
    Split { main, nested }
}

fn fail_row(key: &str, edit: impl FnOnce(&mut FailRow)) -> FailRow {
    let mut r = FailRow {
        key: key.to_owned(),
        label: "label".into(),
        r#where: "Bash".into(),
        piped: 0.0,
        rows: 10.0,
        sessions: 5.0,
        projects: 2.0,
        flagged: 0.0,
        note: None,
        detector: None,
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn time_row(edit: impl FnOnce(&mut TimeRow)) -> TimeRow {
    let mut r = TimeRow {
        key: Job::Test,
        label: "tests",
        runs: 10.0,
        co_present_runs: 12.0,
        total_sec: 3600.0,
        share: 0.5,
        median_sec: 1.0,
        p95_sec: 9.0,
        capped_runs: 0.0,
        capped_sec: 0.0,
        bg_runs: 0.0,
        bg_sec: 0.0,
        samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn cause_row(edit: impl FnOnce(&mut CauseRow)) -> CauseRow {
    let mut r = CauseRow {
        key: "cause:zsh-glob-flag".into(),
        cause: CauseId::ZshGlobFlag,
        kind: CauseKind::Rule,
        label: "unquoted glob in a flag value".into(),
        fix: Some("quote the glob".into()),
        failing: true,
        hits: 40.0,
        flagged: 2.0,
        hits_by: split(10.0, 30.0),
        pairs: 12.0,
        pairs_by: split(2.0, 10.0),
        retry_sessions: 11.0,
        sessions: 30.0,
        projects: 3.0,
        retry_sec: 50.0,
        sec_measured: 12.0,
        retry_ctx_tokens: 900.0,
        tokens_measured: 12.0,
        per1k: split(1.0, 2.0),
        details: Vec::new(),
        diffs: Vec::new(),
        rule: None,
        samples: Vec::new(),
        pair_samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn report(
    bash_sec: f64,
    causes: Vec<CauseRow>,
    fails: Vec<FailRow>,
    time: Vec<TimeRow>,
) -> StatsReport {
    StatsReport {
        generated_at: 0.0,
        window: StatsWindow {
            days: 60.0,
            from_iso: String::new(),
            to_iso: String::new(),
        },
        corpus: CorpusSummary {
            bash_sec,
            ..CorpusSummary::default()
        },
        causes,
        fails,
        time,
        waiting: WaitPanel {
            calls: 0.0,
            total_sec: 0.0,
            share: 0.0,
            sessions: 0.0,
            projects: 0.0,
            median_sec: 0.0,
            p95_sec: 0.0,
            by_shape: Vec::new(),
            bg_runs: 0.0,
            bg_sec: 0.0,
            bg_declared_sec: 0.0,
            samples: Vec::new(),
        },
        clusters: Vec::new(),
        panels: CorpusPanels::default(),
        skipped: Vec::new(),
        caveats: Vec::new(),
        paths: Vec::new(),
    }
}

fn keys(r: &StatsReport) -> Vec<String> {
    findings_of(r).into_iter().map(|f| f.key).collect()
}

fn capped(r: &mut TimeRow) {
    r.capped_runs = 3.0;
    r.capped_sec = 60.0;
}

#[test]
fn time_and_fail_rows_are_never_findings() {
    let r = report(
        100.0,
        vec![],
        vec![
            fail_row("plain", |r| {
                r.note = Some("printed a failure".into());
                r.sessions = 9.0;
            }),
            fail_row("named", |r| {
                r.note = Some("quote the separator".into());
                r.detector = Some("zsh-equals-abort".into());
            }),
        ],
        vec![time_row(capped)],
    );
    assert_eq!(keys(&r), Vec::<String>::new());
}

#[test]
fn a_row_with_no_note_is_never_a_finding() {
    let r = report(
        100.0,
        vec![],
        vec![fail_row("k", |_| {})],
        vec![time_row(|_| {})],
    );
    assert_eq!(findings_of(&r), []);
}

fn piped(key: &str, piped: f64) -> FailRow {
    fail_row(key, |r| {
        r.detector = Some("piped-exit-code".into());
        r.piped = piped;
        r.rows = 10.0;
    })
}

#[test]
fn piped_rows_fold_into_one_finding_keyed_on_the_top_row() {
    let r = report(
        100.0,
        vec![],
        vec![piped("few", 1.0), piped("most", 3.0), piped("some", 2.0)],
        vec![],
    );
    let f = findings_of(&r);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].title, "Piped output hides failed exit codes");
    assert_eq!(f[0].detector.as_deref(), Some("piped-exit-code"));
    assert_eq!(f[0].key, "most");
    assert_eq!(f[0].table, Some(DrillTable::Fail));
    assert_eq!(f[0].impact, "6 calls");
}

#[test]
fn fewer_than_three_piped_calls_make_no_finding() {
    let r = report(
        100.0,
        vec![],
        vec![piped("a", 1.0), piped("b", 1.0)],
        vec![],
    );
    assert_eq!(findings_of(&r), []);
}

fn named_cause(key: &str, id: CauseId, hits: f64) -> CauseRow {
    cause_row(|r| {
        r.key = key.into();
        r.cause = id;
        r.hits = hits;
    })
}

#[test]
fn cause_findings_rank_by_live_hits_and_ties_keep_table_order() {
    let r = report(
        100.0,
        vec![
            named_cause("cause:a", CauseId::ZshGlobFlag, 40.0),
            named_cause("cause:b", CauseId::PkillSelfKill, 40.0),
            named_cause("cause:c", CauseId::ZshGlobPath, 90.0),
        ],
        vec![],
        vec![],
    );
    assert_eq!(keys(&r), ["cause:c", "cause:a", "cause:b"]);
}

#[test]
fn a_dated_rule_ranks_by_hits_after_its_day() {
    let after = |before: f64, n: f64| RuleCheck {
        hits_through: split(before, 0.0),
        hits_after: split(n, 0.0),
        hits_since: split(n, 0.0),
        ..firing()
    };
    let mut a = named_cause("cause:a", CauseId::ZshGlobFlag, 200.0);
    a.rule = Some(after(199.0, 1.0));
    let mut b = named_cause("cause:b", CauseId::PkillSelfKill, 50.0);
    b.rule = Some(after(30.0, 20.0));
    let r = report(100.0, vec![a, b], vec![], vec![]);
    assert_eq!(keys(&r), ["cause:b", "cause:a"]);
}

#[test]
fn a_quiet_dated_rule_with_enough_calls_moves_to_working() {
    let quiet = |calls: f64| RuleCheck {
        hits_through: split(40.0, 0.0),
        hits_after: Split::default(),
        hits_since: Split::default(),
        calls_since: split(calls, calls),
        per1k_since: Split::default(),
        ..firing()
    };
    let mut done = named_cause("cause:a", CauseId::ZshGlobFlag, 40.0);
    done.rule = Some(quiet(2000.0));
    let mut thin = named_cause("cause:b", CauseId::PkillSelfKill, 40.0);
    thin.rule = Some(quiet(10.0));
    let r = report(100.0, vec![done, thin], vec![], vec![]);
    assert_eq!(keys(&r), ["cause:b"]);
    let working: Vec<String> = working_of(&r).into_iter().map(|f| f.key).collect();
    assert_eq!(working, ["cause:a"]);
}

#[test]
fn hits_with_no_date_stay_live_and_keep_a_cause_out_of_working() {
    let mut a = named_cause("cause:a", CauseId::ZshGlobFlag, 50.0);
    a.rule = Some(RuleCheck {
        hits_through: split(40.0, 0.0),
        hits_after: Split::default(),
        hits_since: Split::default(),
        calls_since: split(2000.0, 2000.0),
        per1k_since: Split::default(),
        ..firing()
    });
    let r = report(100.0, vec![a], vec![], vec![]);
    assert_eq!(keys(&r), ["cause:a"]);
    assert!(working_of(&r).is_empty());
    let spread = &findings_of(&r)[0].spread;
    assert!(
        spread.starts_with("40 hits up to the rule (Aug 1), 0 since, 10 with no date"),
        "{spread}"
    );
}

#[test]
fn a_cause_finding_states_its_retries_as_the_cost() {
    let f = findings_of(&report(100.0, vec![cause_row(|_| {})], vec![], vec![]));
    assert_eq!(
        f[0].impact,
        "12 retries (main 2 / sub 10) · 50.0s · 900 ctx tokens re-read"
    );
    assert_eq!(
        f[0].spread,
        "40 hits (38 at exit 0) · 11 sessions with a retry · 3 projects · hits main 10 / sub 30"
    );
}

#[test]
fn a_cause_with_no_measured_gap_states_no_time() {
    let f = findings_of(&report(
        100.0,
        vec![cause_row(|r| {
            r.retry_sec = 0.0;
            r.sec_measured = 0.0;
        })],
        vec![],
        vec![],
    ));
    assert_eq!(
        f[0].impact,
        "12 retries (main 2 / sub 10) · 900 ctx tokens re-read"
    );
}

#[test]
fn a_fix_less_row_is_not_a_finding() {
    let r = report(
        100.0,
        vec![cause_row(|r| {
            r.key = "cause:unclassified:Bash".into();
            r.cause = CauseId::Unclassified;
            r.kind = CauseKind::None;
            r.fix = None;
        })],
        vec![],
        vec![],
    );
    assert_eq!(findings_of(&r), []);
    assert_eq!(NO_RULE, "no rule to suggest");
}

fn firing() -> RuleCheck {
    RuleCheck {
        matches: vec![RuleMatch {
            path: "~/.claude/CLAUDE.md".into(),
            line: 100.0,
            excerpt: "Quote globs".into(),
        }],
        since_iso: Some("2026-08-01T00:00:00.000Z".into()),
        hits_through: Split::default(),
        hits_after: Split::default(),
        hits_since: split(1.0, 9.0),
        calls_since: split(100.0, 100.0),
        per1k_since: split(10.0, 90.0),
        advice: "setopt nonomatch".into(),
    }
}

#[test]
fn a_rule_present_and_firing_says_so_instead_of_the_fix() {
    let row = cause_row(|r| r.rule = Some(firing()));
    assert_eq!(
        cause_note(&row).as_deref(),
        Some(
            "rule in ~/.claude/CLAUDE.md:100 · still firing main 10/1k, sub 90/1k since 2026-08-01: setopt nonomatch"
        )
    );
    assert_eq!(
        rule_line(&RuleCheck {
            calls_since: split(0.0, 100.0),
            ..firing()
        }),
        "rule in ~/.claude/CLAUDE.md:100 · still firing sub 90/1k since 2026-08-01"
    );
    assert_eq!(
        rule_line(&RuleCheck {
            hits_through: Split::default(),
            hits_after: Split::default(),
            hits_since: split(0.0, 0.0),
            ..firing()
        }),
        "rule in ~/.claude/CLAUDE.md:100 · quiet since 2026-08-01"
    );
    let fresh = cause_row(|r| {
        r.rule = Some(RuleCheck {
            hits_through: Split::default(),
            hits_after: Split::default(),
            hits_since: split(0.0, 0.0),
            calls_since: split(0.0, 0.0),
            per1k_since: split(0.0, 0.0),
            advice: "if it keeps firing: setopt nonomatch".into(),
            ..firing()
        });
    });
    assert_eq!(
        cause_note(&fresh).as_deref(),
        Some(
            "rule in ~/.claude/CLAUDE.md:100 · not measured yet (added 2026-08-01): if it keeps firing: setopt nonomatch"
        )
    );
}

#[test]
fn no_suggestion_says_why() {
    assert_eq!(
        no_suggestion_reason(&report(100.0, vec![], vec![], vec![])),
        Some("no rule cause fired in this window")
    );
    assert_eq!(
        no_suggestion_reason(&report(100.0, vec![cause_row(|_| {})], vec![], vec![])),
        None
    );
    let stated = cause_row(|r| {
        r.rule = Some(RuleCheck {
            matches: vec![],
            since_iso: None,
            hits_through: Split::default(),
            hits_after: Split::default(),
            hits_since: split(0.0, 0.0),
            calls_since: split(0.0, 0.0),
            per1k_since: split(0.0, 0.0),
            advice: String::new(),
        });
    });
    assert_eq!(
        no_suggestion_reason(&report(100.0, vec![stated], vec![], vec![])),
        Some("every rule cause that fired is already stated in a memory file")
    );
}

#[test]
fn a_share_of_no_shell_seconds_says_so() {
    assert_eq!(
        time_spread(&time_row(|_| {}), 0.0),
        "not measured · 10 runs"
    );
    assert_eq!(
        time_spread(&time_row(|_| {}), 100.0),
        "50.0% of shell time · 10 runs"
    );
}

#[test]
fn a_ceiling_is_stated_as_a_floor() {
    let note = time_note(&time_row(capped)).unwrap_or_default();
    assert_eq!(
        note,
        "3 of 10 runs hit the harness time limit (1m00s of 1h00m), so this time is a floor"
    );
    let both = time_note(&time_row(|r| {
        capped(r);
        r.bg_runs = 2.0;
    }))
    .unwrap_or_default();
    assert!(
        both.ends_with(". 2 runs went to the background and count only the seconds they ran"),
        "{both}"
    );
    assert_eq!(time_note(&time_row(|_| {})), None);
}

#[test]
fn one_spelling_of_a_per_1k_rate() {
    assert_eq!(rate_text(41.46, 5.0), "41");
    assert_eq!(rate_text(4.236, 5.0), "4.2");
    assert_eq!(rate_text(0.31, 1.0), "0.31");
    assert_eq!(rate_text(0.0, 0.0), "0");
}

#[test]
fn a_side_with_hits_never_reads_zero() {
    assert_eq!(rate_text(0.0, 1.0), "<0.01");
    let line = rule_line(&RuleCheck {
        matches: vec![RuleMatch {
            path: "~/.claude/CLAUDE.md".into(),
            line: 1.0,
            excerpt: "x".into(),
        }],
        since_iso: Some("2026-08-01T00:00:00.000Z".into()),
        hits_through: Split::default(),
        hits_after: Split::default(),
        hits_since: split(0.0, 1.0),
        calls_since: split(10.0, 4000.0),
        per1k_since: split(0.0, 0.25),
        advice: "a".into(),
    });
    assert!(
        line.contains("still firing main 0/1k, sub 0.25/1k"),
        "{line}"
    );
}

#[test]
fn a_cause_finding_carries_its_suggested_rule() {
    let r = report(
        100.0,
        vec![
            cause_row(|_| {}),
            cause_row(|r| {
                r.key = "cause:stated".into();
                r.rule = Some(firing());
            }),
        ],
        vec![],
        vec![],
    );
    let rules: Vec<Option<String>> = findings_of(&r).into_iter().map(|f| f.rule).collect();
    assert_eq!(rules, [Some("quote the glob".to_owned()), None]);
}
