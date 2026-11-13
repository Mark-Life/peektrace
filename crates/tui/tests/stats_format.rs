//! The stats screen's findings and note strip in the TUI number format:
//! tier order, rows with no note, the measured note, and cause drills.

use peektrace_core::stats::accumulate::DrillTable;
use peektrace_core::stats::detectors::causes::CauseKind;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::rules::Split;
use peektrace_core::stats::detectors::timing::TimeRow;
use peektrace_core::stats::report::StatsReport;
use peektrace_core::stats::schema::{CauseId, Job};
use peektrace_tui::screens::stats::text::{Pane, finding_target, findings_of, note_lines_of};

fn fail_row(key: &str, edit: impl FnOnce(&mut FailRow)) -> FailRow {
    let mut r = FailRow {
        key: key.to_owned(),
        label: key.to_owned(),
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
        hits_by: Split {
            main: 10.0,
            nested: 30.0,
        },
        pairs: 12.0,
        pairs_by: Split {
            main: 2.0,
            nested: 10.0,
        },
        retry_sessions: 11.0,
        sessions: 30.0,
        projects: 3.0,
        retry_sec: 50.0,
        sec_measured: 12.0,
        retry_ctx_tokens: 9000.0,
        tokens_measured: 12.0,
        per1k: Split {
            main: 1.0,
            nested: 2.0,
        },
        details: Vec::new(),
        diffs: Vec::new(),
        rule: None,
        samples: Vec::new(),
        pair_samples: Vec::new(),
    };
    edit(&mut r);
    r
}

fn report(causes: Vec<CauseRow>, fails: Vec<FailRow>, time: Vec<TimeRow>) -> StatsReport {
    let mut r = StatsReport {
        causes,
        fails,
        time,
        ..StatsReport::default()
    };
    r.window.days = 30.0;
    r.corpus.transcripts = 1.0;
    r.corpus.bash_sec = 100.0;
    r.corpus.tool_sec = 100.0;
    r
}

fn keys(r: &StatsReport) -> Vec<String> {
    findings_of(r).into_iter().map(|f| f.key).collect()
}

#[test]
fn findings_rank_by_effort_tier_never_by_a_blended_score() {
    let r = report(
        vec![],
        vec![
            fail_row("plain", |r| {
                r.note = Some("measured".into());
                r.sessions = 99.0;
            }),
            fail_row("named", |r| {
                r.note = Some("quote the separator".into());
                r.detector = Some("zsh-equals-abort".into());
                r.sessions = 1.0;
            }),
            fail_row("quiet", |r| r.sessions = 500.0),
        ],
        vec![time_row(|r| {
            r.capped_runs = 3.0;
            r.capped_sec = 60.0;
        })],
    );
    assert_eq!(keys(&r), ["named", "plain", "test"]);
}

#[test]
fn a_row_core_minted_no_note_for_is_never_a_finding() {
    let r = report(vec![], vec![fail_row("k", |_| {})], vec![time_row(|_| {})]);
    assert_eq!(findings_of(&r), []);
}

#[test]
fn the_note_strip_states_the_measurement_never_invented_advice() {
    let r = report(
        vec![],
        vec![fail_row("k", |r| r.rows = 7.0)],
        vec![time_row(|r| {
            r.capped_runs = 2.0;
            r.capped_sec = 30.0;
        })],
    );
    let no_rule = note_lines_of(&[], 0, Pane::Fails, &r);
    assert!(no_rule[0].contains("no rule to suggest"), "{no_rule:?}");
    let capped = note_lines_of(&[], 0, Pane::Time, &r);
    assert!(capped[0].contains("floor"), "{capped:?}");
    assert!(capped[1].contains("median"), "{capped:?}");
}

#[test]
fn a_cause_finding_leads_and_drills_into_the_cause_table() {
    let r = report(
        vec![cause_row(|_| {})],
        vec![fail_row("named", |r| {
            r.note = Some("x".into());
            r.detector = Some("piped-exit-code".into());
        })],
        vec![],
    );
    let found = findings_of(&r);
    let found_keys: Vec<&str> = found.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(found_keys, ["cause:zsh-glob-flag", "named"]);
    let target = finding_target(&found[0], &r).expect("target");
    assert_eq!(target.table, DrillTable::Cause);
    assert_eq!(target.key, "cause:zsh-glob-flag");
    assert_eq!(target.note.as_deref(), Some("quote the glob"));
    assert!(
        target.spread.contains("11 sessions with a retry"),
        "{}",
        target.spread
    );
    assert!(
        target.spread.contains("12 retries (main"),
        "{}",
        target.spread
    );
}

#[test]
fn a_cause_finding_spells_re_read_context_compactly() {
    let r = report(
        vec![cause_row(|r| r.retry_ctx_tokens = 32_389_802.0)],
        vec![],
        vec![],
    );
    let found = findings_of(&r);
    assert_eq!(
        found[0].impact,
        "12 retries (main 2 / sub 10) · 50.0s · 32.4M ctx tokens re-read"
    );
}
