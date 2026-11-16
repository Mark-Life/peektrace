//! What a cold start leads with: rows that name a change, with what they
//! cost and how far they reach. A cause its rule already stopped moves to a
//! separate "working" list instead of ranking with live ones.

use super::accumulate::{DrillTable, SampleRef};
use super::cost_findings::cost_findings;
use super::detectors::causes::CauseKind;
use super::detectors::failures::{FailRow, PIPED_EXIT_CODE};
use super::detectors::retries::CauseRow;
use super::detectors::rules::{RuleCheck, Split};
use super::detectors::timing::TimeRow;
use super::format::{count, hms, percent, rate_text, tokens};
use super::service::StatsReport;

pub const NO_RULE: &str = "no rule to suggest";

/// How a surface spells the numbers in the sentences findings compose.
#[derive(Debug, Clone, Copy)]
pub struct FindingFormat {
    pub count: fn(f64) -> String,
    pub hms: fn(f64) -> String,
    pub pct: fn(f64) -> String,
    pub tokens: fn(f64) -> String,
}

impl FindingFormat {
    /// The CLI's spelling: `12.3k` tokens.
    pub const CLI: Self = Self {
        count,
        hms,
        pct: percent,
        tokens,
    };
}

/// One finding: a row that names a change, plus the route back.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub detector: Option<String>,
    pub impact: String,
    pub key: String,
    pub note: String,
    pub spread: String,
    pub title: String,
    /// The memory line a cause suggests ([`suggested_rule_of`]); `None` for other rows.
    pub rule: Option<String>,
    /// The table the row came from; `None` for a cost finding, which has no
    /// drill.
    pub table: Option<DrillTable>,
    pub samples: Vec<SampleRef>,
}

pub fn split_of(s: Split) -> String {
    split_with(s, &FindingFormat::CLI)
}

fn split_with(s: Split, f: &FindingFormat) -> String {
    format!("main {} / sub {}", (f.count)(s.main), (f.count)(s.nested))
}

pub fn cause_spread(row: &CauseRow) -> String {
    cause_spread_with(row, &FindingFormat::CLI)
}

pub fn cause_spread_with(row: &CauseRow, f: &FindingFormat) -> String {
    let exit0 = row.hits - row.flagged;
    let hits = if let Some(split) = rule_split(row, f) {
        split
    } else if exit0 > 0.0 {
        format!(
            "{} hits ({} at exit 0)",
            (f.count)(row.hits),
            (f.count)(exit0)
        )
    } else {
        format!("{} hits", (f.count)(row.hits))
    };
    let retries = if row.failing {
        format!(" · {} sessions with a retry", (f.count)(row.retry_sessions))
    } else {
        String::new()
    };
    format!(
        "{hits}{retries} · {} projects · hits {}",
        (f.count)(row.projects),
        split_with(row.hits_by, f)
    )
}

pub fn cause_impact(row: &CauseRow) -> String {
    cause_impact_with(row, &FindingFormat::CLI)
}

pub fn cause_impact_with(row: &CauseRow, f: &FindingFormat) -> String {
    if row.pairs == 0.0 {
        return format!("{} sessions", (f.count)(row.sessions));
    }
    let what = if row.failing { "retries" } else { "read-backs" };
    let toks = if row.tokens_measured > 0.0 {
        format!(" · {} ctx tokens re-read", (f.tokens)(row.retry_ctx_tokens))
    } else {
        String::new()
    };
    let sec = if row.sec_measured == 0.0 {
        String::new()
    } else {
        format!(" · {}", (f.hms)(row.retry_sec))
    };
    format!(
        "{} {what} ({}){sec}{toks}",
        (f.count)(row.pairs),
        split_with(row.pairs_by, f)
    )
}

fn rule_place(rule: &RuleCheck) -> String {
    rule.matches.first().map_or_else(
        || "memory".to_owned(),
        |m| format!("{}:{}", m.path, crate::js::num::to_js_string(m.line)),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleState {
    Firing,
    Quiet,
    Unmeasured,
}

pub fn rule_state(rule: &RuleCheck) -> RuleState {
    if rule.calls_since.total() == 0.0 {
        RuleState::Unmeasured
    } else if rule.hits_since.total() == 0.0 {
        RuleState::Quiet
    } else {
        RuleState::Firing
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Oct 21` from an ISO stamp.
fn short_day(iso: &str) -> String {
    let month = iso
        .get(5..7)
        .and_then(|m| m.parse::<usize>().ok())
        .and_then(|m| MONTHS.get(m.wrapping_sub(1)));
    let day = iso.get(8..10).and_then(|d| d.parse::<u32>().ok());
    match (month, day) {
        (Some(m), Some(d)) => format!("{m} {d}"),
        _ => crate::js::units::slice(iso, 0, 10),
    }
}

/// `72 hits up to the rule (Oct 21), 0 since`, when the rule has a date;
/// hits with no day are named apart.
pub fn rule_split(row: &CauseRow, f: &FindingFormat) -> Option<String> {
    let rule = row.rule.as_ref()?;
    let day = short_day(rule.since_iso.as_deref()?);
    let none = undated(row, rule);
    let tail = if none > 0.0 {
        format!(", {} with no date", (f.count)(none))
    } else {
        String::new()
    };
    Some(format!(
        "{} hits up to the rule ({day}), {} since{tail}",
        (f.count)(rule.hits_through.total()),
        (f.count)(rule.hits_after.total())
    ))
}

/// Hits with no day: neither before nor after the rule.
fn undated(row: &CauseRow, rule: &RuleCheck) -> f64 {
    (row.hits - rule.hits_through.total() - rule.hits_after.total()).max(0.0)
}

/// Hits a fix could still save: those after the rule's day plus any with no
/// day, or all of them when no dated rule covers the cause.
pub fn live_hits(row: &CauseRow) -> f64 {
    match &row.rule {
        Some(r) if r.since_iso.is_some() && rule_state(r) != RuleState::Unmeasured => {
            r.hits_after.total() + undated(row, r)
        }
        _ => row.hits,
    }
}

/// Hits the rule's calls since would have shown at the window's rate, below
/// which "quiet" proves nothing.
const QUIET_EVIDENCE: f64 = 3.0;

/// The rule stopped the cause: no hit anywhere since its day, over enough
/// calls that the old rate would have shown some.
pub fn is_working(row: &CauseRow) -> bool {
    row.rule.as_ref().is_some_and(|r| {
        let expected = (row.per1k.main * r.calls_since.main
            + row.per1k.nested * r.calls_since.nested)
            / 1000.0;
        r.since_iso.is_some()
            && rule_state(r) == RuleState::Quiet
            && r.hits_after.total() + undated(row, r) == 0.0
            && expected >= QUIET_EVIDENCE
    })
}

/// `rule in <path>:<line> · quiet since D`, and the other two states.
pub fn rule_line(rule: &RuleCheck) -> String {
    let day = rule
        .since_iso
        .as_deref()
        .map(|s| crate::js::units::slice(s, 0, 10));
    let since = day
        .as_ref()
        .map_or_else(String::new, |d| format!(" since {d}"));
    let place = format!("rule in {}", rule_place(rule));
    match rule_state(rule) {
        RuleState::Unmeasured => {
            let added = day
                .as_ref()
                .map_or_else(String::new, |d| format!(" (added {d})"));
            format!("{place} · not measured yet{added}")
        }
        RuleState::Quiet => format!("{place} · quiet{since}"),
        RuleState::Firing => {
            let sides = [
                (
                    "main",
                    rule.calls_since.main,
                    rule.hits_since.main,
                    rule.per1k_since.main,
                ),
                (
                    "sub",
                    rule.calls_since.nested,
                    rule.hits_since.nested,
                    rule.per1k_since.nested,
                ),
            ];
            let rates = sides
                .iter()
                .filter(|s| s.1 > 0.0)
                .map(|(side, _, hits, rate)| format!("{side} {}/1k", rate_text(*rate, *hits)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{place} · still firing {rates}{since}")
        }
    }
}

pub fn cause_note(row: &CauseRow) -> Option<String> {
    let fix = row.fix.as_ref()?;
    Some(match &row.rule {
        Some(r) if rule_state(r) != RuleState::Quiet => format!("{}: {}", rule_line(r), r.advice),
        _ => fix.clone(),
    })
}

pub fn suggested_rule_of(row: &CauseRow) -> Option<&str> {
    (row.kind == CauseKind::Rule && row.rule.is_none() && row.hits > 0.0)
        .then_some(row.fix.as_deref())
        .flatten()
}

pub fn suggested_rules(report: &StatsReport) -> Vec<&str> {
    report.causes.iter().filter_map(suggested_rule_of).collect()
}

pub fn no_suggestion_reason(report: &StatsReport) -> Option<&'static str> {
    if !suggested_rules(report).is_empty() {
        return None;
    }
    let fired = report
        .causes
        .iter()
        .any(|r| r.kind == CauseKind::Rule && r.fix.is_some() && r.hits > 0.0);
    Some(if fired {
        "every rule cause that fired is already stated in a memory file"
    } else {
        "no rule cause fired in this window"
    })
}

pub fn fail_spread(row: &FailRow) -> String {
    fail_spread_with(row, &FindingFormat::CLI)
}

pub fn fail_spread_with(row: &FailRow, f: &FindingFormat) -> String {
    format!(
        "{} calls · {} projects · {} flagged",
        (f.count)(row.rows),
        (f.count)(row.projects),
        (f.count)(row.flagged)
    )
}

pub fn time_spread(row: &TimeRow, bash_sec: f64) -> String {
    time_spread_with(row, bash_sec, &FindingFormat::CLI)
}

pub fn time_spread_with(row: &TimeRow, bash_sec: f64, f: &FindingFormat) -> String {
    if bash_sec > 0.0 {
        format!(
            "{} of shell time · {} runs",
            (f.pct)(row.share),
            (f.count)(row.runs)
        )
    } else {
        format!("not measured · {} runs", (f.count)(row.runs))
    }
}

pub fn time_note(row: &TimeRow) -> Option<String> {
    time_note_with(row, &FindingFormat::CLI)
}

pub fn time_note_with(row: &TimeRow, f: &FindingFormat) -> Option<String> {
    let capped = (row.capped_runs > 0.0).then(|| {
        format!(
            "{} of {} runs hit the harness time limit ({} of {}), so this time is a floor",
            (f.count)(row.capped_runs),
            (f.count)(row.runs),
            (f.hms)(row.capped_sec),
            (f.hms)(row.total_sec)
        )
    });
    let bg = (row.bg_runs > 0.0).then(|| {
        format!(
            "{} runs went to the background and count only the seconds they ran",
            (f.count)(row.bg_runs)
        )
    });
    let notes: Vec<String> = capped.into_iter().chain(bg).collect();
    (!notes.is_empty()).then(|| notes.join(". "))
}

fn cause_finding(row: &CauseRow, note: String, f: &FindingFormat) -> Finding {
    Finding {
        detector: Some(row.cause.as_str().to_owned()),
        impact: cause_impact_with(row, f),
        key: row.key.clone(),
        note,
        spread: cause_spread_with(row, f),
        title: row.label.clone(),
        rule: suggested_rule_of(row).map(str::to_owned),
        table: Some(DrillTable::Cause),
        samples: row.samples.clone(),
    }
}

/// Fewest piped calls worth a finding.
const PIPED_MIN: f64 = 3.0;
const PIPED_LABELS: usize = 3;

/// Every fail row whose output was piped, folded into one change.
fn piped_finding(report: &StatsReport, f: &FindingFormat) -> Option<Finding> {
    let mut rows: Vec<&FailRow> = report
        .fails
        .iter()
        .filter(|r| r.detector.as_deref() == Some(PIPED_EXIT_CODE))
        .collect();
    let piped: f64 = rows.iter().map(|r| r.piped).sum();
    if piped < PIPED_MIN {
        return None;
    }
    rows.sort_by(|a, b| super::rank::desc(a.piped, b.piped));
    let top = rows.first()?;
    let labels: Vec<&str> = rows
        .iter()
        .take(PIPED_LABELS)
        .map(|r| r.label.as_str())
        .collect();
    Some(Finding {
        detector: Some(PIPED_EXIT_CODE.to_owned()),
        impact: format!("{} calls", (f.count)(piped)),
        key: top.key.clone(),
        note: "These commands printed a failure but exited 0, because the exit code came from head, tail or grep at the end of the pipe. Add `set -o pipefail`, or do not pipe test output.".to_owned(),
        spread: format!(
            "{} of {} failing calls piped · {}",
            (f.count)(piped),
            (f.count)(rows.iter().map(|r| r.rows).sum()),
            labels.join(", ")
        ),
        title: "Piped output hides failed exit codes".to_owned(),
        rule: None,
        table: Some(DrillTable::Fail),
        samples: top.samples.clone(),
    })
}

/// Live cause findings, most live hits first; ties keep the table's order.
fn live_causes(report: &StatsReport, f: &FindingFormat) -> Vec<Finding> {
    let mut rows: Vec<(&CauseRow, String)> = report
        .causes
        .iter()
        .filter(|row| row.hits > 0.0 && !is_working(row))
        .filter_map(|row| Some((row, cause_note(row)?)))
        .collect();
    rows.sort_by(|a, b| super::rank::desc(live_hits(a.0), live_hits(b.0)));
    rows.into_iter()
        .map(|(row, note)| cause_finding(row, note, f))
        .collect()
}

/// Causes a rule already stopped, for the collapsed "working" list.
pub fn working_with(report: &StatsReport, f: &FindingFormat) -> Vec<Finding> {
    report
        .causes
        .iter()
        .filter(|row| is_working(row))
        .filter_map(|row| {
            let rule = row.rule.as_ref()?;
            Some(cause_finding(row, rule_line(rule), f))
        })
        .collect()
}

pub fn working_of(report: &StatsReport) -> Vec<Finding> {
    working_with(report, &FindingFormat::CLI)
}

/// Cost findings that hold a large share of the spend, then live causes,
/// then the piped-exit finding, then the smaller cost findings.
pub fn findings_of(report: &StatsReport) -> Vec<Finding> {
    findings_with(report, &FindingFormat::CLI)
}

/// [`findings_of`] with a surface's own number format.
pub fn findings_with(report: &StatsReport, f: &FindingFormat) -> Vec<Finding> {
    let (lead, rest) = cost_findings(report, f);
    lead.into_iter()
        .chain(live_causes(report, f))
        .chain(piped_finding(report, f))
        .chain(rest)
        .collect()
}
