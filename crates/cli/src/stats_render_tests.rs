//! The plain-text stats views over synthetic results: the drill header, the
//! report's order and fix wording, and the usage table.

use peektrace_core::Agent;
use peektrace_core::js::regex::Re;
use peektrace_core::stats::accumulate::{
    CorpusSummary, DrillHit, DrillTable, SampleRef, StatsWindow,
};
use peektrace_core::stats::detectors::causes::CauseKind;
use peektrace_core::stats::detectors::corpus::CorpusPanels;
use peektrace_core::stats::detectors::failures::FailRow;
use peektrace_core::stats::detectors::retries::CauseRow;
use peektrace_core::stats::detectors::rules::{RuleCheck, RuleMatch, Split};
use peektrace_core::stats::detectors::usage::{
    Agg, UsageBlock, UsageBy, UsageResult, UsageRow, UsageSkipped, UsageTotals,
};
use peektrace_core::stats::detectors::waiting::WaitPanel;
use peektrace_core::stats::findings::suggested_rules;
use peektrace_core::stats::schema::{CauseId, Kind};
use peektrace_core::stats::service::{DrillResult, StatsReport};
use peektrace_core::stats::usage_service::UsageReport;

use crate::stats_render::{render_drill, render_report, render_usage};

fn hit() -> DrillHit {
    DrillHit {
        sample: SampleRef {
            sid: "s1".into(),
            agent: Agent::Claude,
            project: "demo".into(),
            kind: Kind::Main,
            parent: None,
            seq: 1.0,
            tool_use_id: None,
            ts: Some("2026-03-01T10:00:00.000Z".into()),
            line: "bun test".into(),
        },
        dur_sec: Some(2.0),
        tool: "Bash".into(),
        failed: true,
        pair_tool_use_id: None,
        pair_line: None,
    }
}

fn header_of(offset: f64, hits: Vec<DrillHit>) -> String {
    let d = DrillResult {
        table: DrillTable::Fail,
        key: "k".into(),
        total: 12,
        offset,
        hits,
        paths: Vec::new(),
    };
    render_drill(&d, false)
        .split('\n')
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn drill_first_page_keeps_the_short_header() {
    assert_eq!(header_of(0.0, vec![hit(), hit()]), "fail k: 2 of 12 calls");
}

#[test]
fn drill_later_page_names_its_range() {
    assert_eq!(
        header_of(5.0, vec![hit(), hit()]),
        "fail k: calls 6-7 of 12"
    );
}

#[test]
fn drill_offset_past_the_end_says_so() {
    assert_eq!(
        header_of(20.0, vec![]),
        "fail k: 0 of 12 calls (offset 20 is past the end)"
    );
}

fn split(main: f64, nested: f64) -> Split {
    Split { main, nested }
}

fn cause_row(edit: impl FnOnce(&mut CauseRow)) -> CauseRow {
    let mut r = CauseRow {
        key: "cause:zsh-glob-flag".into(),
        cause: CauseId::ZshGlobFlag,
        kind: CauseKind::Rule,
        label: "unquoted glob in a flag value".into(),
        fix: Some("quote every glob passed as a flag value".into()),
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
        retry_ctx_tokens: 9000.0,
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

fn still_firing() -> CauseRow {
    cause_row(|r| {
        r.key = "cause:zsh-equals-abort".into();
        r.cause = CauseId::ZshEqualsAbort;
        r.label = "unquoted echo === separator".into();
        r.fix = Some("quote the separator: echo \"===\"".into());
        r.rule = Some(RuleCheck {
            matches: vec![RuleMatch {
                path: "~/.claude/CLAUDE.md".into(),
                line: 99.0,
                excerpt: "x".into(),
            }],
            since_iso: Some("2026-08-01T00:00:00.000Z".into()),
            hits_since: split(1.0, 3.0),
            calls_since: split(500.0, 500.0),
            per1k_since: split(2.0, 6.0),
            advice: "unsetopt equals".into(),
        });
    })
}

fn fail_row() -> FailRow {
    FailRow {
        key: "flag:Bash".into(),
        label: "Bash errors".into(),
        r#where: "Bash".into(),
        rows: 3.0,
        sessions: 3.0,
        projects: 1.0,
        flagged: 3.0,
        note: None,
        detector: None,
        samples: Vec::new(),
    }
}

fn report(causes: Vec<CauseRow>, fails: Vec<FailRow>) -> StatsReport {
    StatsReport {
        generated_at: 0.0,
        window: StatsWindow {
            days: 30.0,
            from_iso: "a".into(),
            to_iso: "b".into(),
        },
        corpus: CorpusSummary {
            transcripts: 1.0,
            sessions: 1.0,
            projects: 1.0,
            tool_calls: 1.0,
            bash_calls: 1.0,
            bash_sec: 100.0,
            tool_sec: 100.0,
            ..CorpusSummary::default()
        },
        causes,
        fails,
        time: Vec::new(),
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

#[test]
fn report_findings_lead_then_causes_then_fails() {
    let text = render_report(
        &report(vec![cause_row(|_| {}), still_firing()], vec![fail_row()]),
        false,
    );
    let at = |s: &str| text.find(s).unwrap_or(usize::MAX);
    assert!(at("FIX FIRST") > 0 && at("FIX FIRST") != usize::MAX);
    assert!(at("FIX FIRST") < at("AVOIDABLE RETRIES"));
    assert!(at("AVOIDABLE RETRIES") < at("WHAT FAILS"));
    assert!(text.contains("--cause cause:zsh-glob-flag"));
    assert!(text.contains(
        "rule in ~/.claude/CLAUDE.md:99 · still firing main 2/1k, sub 6/1k since 2026-08-01: unsetopt equals"
    ));
    assert!(text.contains("\thits m/s\tper 1k in window m/s\t"));
    assert!(text.contains("9.0k ctx tokens re-read"));
    assert!(!text.contains("9,000"));
    assert!(text.contains("no rule to suggest"));
}

#[test]
fn report_suggests_only_unstated_rule_causes() {
    let r = report(
        vec![
            cause_row(|_| {}),
            still_firing(),
            cause_row(|r| {
                r.key = "cause:command-not-found".into();
                r.cause = CauseId::CommandNotFound;
                r.kind = CauseKind::Env;
                r.fix = Some("install jq".into());
            }),
            cause_row(|r| {
                r.key = "cause:unclassified:Bash".into();
                r.cause = CauseId::Unclassified;
                r.kind = CauseKind::None;
                r.fix = None;
            }),
        ],
        vec![],
    );
    assert_eq!(
        suggested_rules(&r),
        ["quote every glob passed as a flag value"]
    );
    assert!(render_report(&r, false).contains(
        "SUGGESTED RULES  (copy into CLAUDE.md / AGENTS.md; causes already stated are not repeated)\n- quote every glob passed as a flag value"
    ));
}

#[test]
fn report_no_suggestion_says_why() {
    assert!(
        render_report(&report(vec![], vec![]), false)
            .contains("SUGGESTED RULES  none: no rule cause fired in this window")
    );
    assert!(
        render_report(&report(vec![still_firing()], vec![]), false).contains(
            "SUGGESTED RULES  none: every rule cause that fired is already stated in a memory file"
        )
    );
}

fn totals(models: &[&str]) -> UsageTotals {
    UsageTotals {
        agg: Agg {
            turns: 3.0,
            usd: 1.234,
            input: 1200.0,
            output: 300.0,
            cache_read: 2_500_000.0,
            cache_creation: 40_000.0,
        },
        total_tokens: 2_541_500.0,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
    }
}

fn usage_row(key: &str, models: &[&str], block: Option<UsageBlock>) -> UsageRow {
    UsageRow {
        key: key.into(),
        totals: totals(models),
        block,
    }
}

fn usage_report(by: UsageBy, tz: &str, rows: Vec<UsageRow>) -> UsageReport {
    UsageReport {
        generated_at: 0.0,
        by,
        tz: tz.into(),
        days: 30,
        from_ms: 0,
        now_ms: 0,
        result: UsageResult {
            caveats: vec!["Cost is an ESTIMATE".into()],
            chain_cut: false,
            rows,
            skipped: UsageSkipped::default(),
            totals: totals(&["claude-sonnet-4-5"]),
            unpriced_models: Vec::new(),
        },
    }
}

fn day_row() -> UsageRow {
    usage_row("2026-08-01", &["claude-sonnet-4-5"], None)
}

#[test]
fn usage_day_rows_carry_tokens_cost_and_totals() {
    let out = render_usage(&usage_report(UsageBy::Day, "UTC", vec![day_row()]), true);
    let lines: Vec<&str> = out.split('\n').collect();
    let header = Re::new(
        r"^DATE\s+INPUT\s+OUTPUT\s+CACHE W\s+CACHE R\s+TOTAL\s+COST\s+MODELS$",
        "",
    );
    assert!(header.is_match(lines[1]), "{out}");
    for want in [
        "2026-08-01",
        "2.5M",
        "~$1.23",
        "NOTES\n- Cost is an ESTIMATE",
    ] {
        assert!(out.contains(want), "{want}");
    }
    assert!(lines.iter().any(|l| l.starts_with("TOTAL ")));
}

#[test]
fn usage_block_rows_print_in_the_report_zone() {
    let block = UsageBlock {
        start_iso: "2026-08-01T10:00:00.000Z".into(),
        end_iso: "2026-08-01T15:00:00.000Z".into(),
        last_iso: "2026-08-01T11:00:00.000Z".into(),
        active: true,
    };
    let row = usage_row(
        "2026-08-01T10:00:00.000Z",
        &["claude-sonnet-4-5"],
        Some(block),
    );
    let out = render_usage(
        &usage_report(UsageBy::Block, "Asia/Kolkata", vec![row]),
        true,
    );
    assert!(
        Re::new(r"BLOCK START\s+END\s+ACTIVE", "").is_match(&out),
        "{out}"
    );
    assert!(
        Re::new(r"2026-08-01 15:30\s+20:30\s+\*", "").is_match(&out),
        "{out}"
    );
}

#[test]
fn usage_long_model_list_names_three() {
    let row = usage_row("2026-08-01", &["a", "b", "c", "d", "e"], None);
    let out = render_usage(&usage_report(UsageBy::Day, "UTC", vec![row]), true);
    assert!(out.contains("a,b,c +2"), "{out}");
}

#[test]
fn usage_compact_is_tab_separated_and_empty_has_no_totals() {
    let out = render_usage(&usage_report(UsageBy::Day, "UTC", vec![]), false);
    assert!(out.contains("date\tinput\toutput"), "{out}");
    assert!(out.contains("(none)"));
    assert!(!out.contains("TOTAL\t"));
}
