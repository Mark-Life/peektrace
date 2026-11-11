//! The cold-start list: tier order, the sentence each row takes, and a share
//! of nothing that says so. TS injects stand-in number formatters; Rust has
//! one set, so the expected strings use the real ones (`50.0s`, `900`).
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

use peektrace_core::stats::accumulate::{CorpusSummary, StatsWindow};
use peektrace_core::stats::detectors::causes::CauseKind;
use peektrace_core::stats::detectors::corpus::CorpusPanels;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::rules::{RuleCheck, RuleMatch, Split};
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::detectors::waiting::WaitPanel;
use peektrace_core::stats::findings::{
    NO_RULE, cause_note, findings_of, no_suggestion_reason, rule_line, time_note, time_spread,
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
fn a_named_rule_leads_then_a_note_then_time_rows() {
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
            fail_row("quiet", |_| {}),
        ],
        vec![time_row(capped)],
    );
    assert_eq!(keys(&r), ["named", "plain", "test"]);
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

#[test]
fn the_two_tables_keep_their_own_units() {
    let r = report(
        100.0,
        vec![],
        vec![fail_row("k", |r| {
            r.note = Some("printed a failure".into());
            r.sessions = 9.0;
        })],
        vec![time_row(capped)],
    );
    let f = findings_of(&r);
    assert_eq!(f[0].impact, "9 sessions");
    assert_eq!(f[1].impact, "1h00m");
}

#[test]
fn cause_findings_lead_in_the_cause_tables_order() {
    let r = report(
        100.0,
        vec![
            cause_row(|_| {}),
            cause_row(|r| {
                r.key = "cause:pkill-self-kill".into();
                r.cause = CauseId::PkillSelfKill;
            }),
        ],
        vec![fail_row("named", |r| {
            r.note = Some("x".into());
            r.detector = Some("piped-exit-code".into());
        })],
        vec![time_row(capped)],
    );
    assert_eq!(
        keys(&r),
        [
            "cause:zsh-glob-flag",
            "cause:pkill-self-kill",
            "named",
            "test"
        ]
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
            hits_since: split(0.0, 0.0),
            ..firing()
        }),
        "rule in ~/.claude/CLAUDE.md:100 · quiet since 2026-08-01"
    );
    let fresh = cause_row(|r| {
        r.rule = Some(RuleCheck {
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
    assert!(note.contains("harness ceiling") && note.contains("these seconds are a floor"));
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
