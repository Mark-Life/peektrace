//! What a cold start leads with: rows that name a change, with what they
//! cost and how far they reach.

use super::accumulate::{DrillTable, SampleRef};
use super::detectors::causes::CauseKind;
use super::detectors::failures::FailRow;
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
    /// The table the row came from: cause, fail or time.
    pub table: DrillTable,
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
    let hits = if exit0 > 0.0 {
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
    if row.capped_runs > 0.0 {
        return Some(format!(
            "{} of {} runs returned at a harness ceiling ({} of {}): these seconds are a floor",
            (f.count)(row.capped_runs),
            (f.count)(row.runs),
            (f.hms)(row.capped_sec),
            (f.hms)(row.total_sec)
        ));
    }
    (row.bg_runs > 0.0).then(|| {
        format!(
            "{} runs were backgrounded and charged only the seconds they took, which understates the wait they started",
            (f.count)(row.bg_runs)
        )
    })
}

/// Cause findings first, then fail rows with a detector, then the other
/// fail rows with a note, then time rows with a note.
pub fn findings_of(report: &StatsReport) -> Vec<Finding> {
    findings_with(report, &FindingFormat::CLI)
}

/// [`findings_of`] with a surface's own number format.
pub fn findings_with(report: &StatsReport, f: &FindingFormat) -> Vec<Finding> {
    let causes = report.causes.iter().filter_map(|row| {
        let note = cause_note(row).filter(|_| row.hits > 0.0)?;
        Some(Finding {
            detector: Some(row.cause.as_str().to_owned()),
            impact: cause_impact_with(row, f),
            key: row.key.clone(),
            note,
            spread: cause_spread_with(row, f),
            title: row.label.clone(),
            table: DrillTable::Cause,
            samples: row.samples.clone(),
        })
    });
    let fails: Vec<Finding> = report
        .fails
        .iter()
        .filter_map(|row| {
            Some(Finding {
                detector: row.detector.clone(),
                impact: format!("{} sessions", (f.count)(row.sessions)),
                key: row.key.clone(),
                note: row.note.clone()?,
                spread: fail_spread_with(row, f),
                title: row.label.clone(),
                table: DrillTable::Fail,
                samples: row.samples.clone(),
            })
        })
        .collect();
    let times = report.time.iter().filter_map(|row| {
        Some(Finding {
            detector: None,
            impact: (f.hms)(row.total_sec),
            key: row.key.as_str().to_owned(),
            note: time_note_with(row, f)?,
            spread: time_spread_with(row, report.corpus.bash_sec, f),
            title: row.label.to_owned(),
            table: DrillTable::Time,
            samples: row.samples.clone(),
        })
    });
    causes
        .chain(fails.iter().filter(|f| f.detector.is_some()).cloned())
        .chain(fails.iter().filter(|f| f.detector.is_none()).cloned())
        .chain(times)
        .collect()
}
