//! What a cold start leads with: rows that name a change, with what they
//! cost and how far they reach.

use super::detectors::causes::CauseKind;
use super::detectors::failures::FailRow;
use super::detectors::retries::CauseRow;
use super::detectors::rules::{RuleCheck, Split};
use super::detectors::timing::TimeRow;
use super::format::{count, hms, percent, rate_text, tokens};
use super::service::StatsReport;

pub const NO_RULE: &str = "no rule to suggest";

/// One finding: a row that names a change, plus the route back.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub detector: Option<String>,
    pub impact: String,
    pub key: String,
    pub note: String,
    pub spread: String,
    pub title: String,
}

pub fn split_of(s: Split) -> String {
    format!("main {} / sub {}", count(s.main), count(s.nested))
}

pub fn cause_spread(row: &CauseRow) -> String {
    let exit0 = row.hits - row.flagged;
    let hits = if exit0 > 0.0 {
        format!("{} hits ({} at exit 0)", count(row.hits), count(exit0))
    } else {
        format!("{} hits", count(row.hits))
    };
    let retries = if row.failing {
        format!(" · {} sessions with a retry", count(row.retry_sessions))
    } else {
        String::new()
    };
    format!(
        "{hits}{retries} · {} projects · hits {}",
        count(row.projects),
        split_of(row.hits_by)
    )
}

pub fn cause_impact(row: &CauseRow) -> String {
    if row.pairs == 0.0 {
        return format!("{} sessions", count(row.sessions));
    }
    let what = if row.failing { "retries" } else { "read-backs" };
    let toks = if row.tokens_measured > 0.0 {
        format!(" · {} ctx tokens re-read", tokens(row.retry_ctx_tokens))
    } else {
        String::new()
    };
    let sec = if row.sec_measured == 0.0 {
        String::new()
    } else {
        format!(" · {}", hms(row.retry_sec))
    };
    format!(
        "{} {what} ({}){sec}{toks}",
        count(row.pairs),
        split_of(row.pairs_by)
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
    format!(
        "{} calls · {} projects · {} flagged",
        count(row.rows),
        count(row.projects),
        count(row.flagged)
    )
}

pub fn time_spread(row: &TimeRow, bash_sec: f64) -> String {
    if bash_sec > 0.0 {
        format!(
            "{} of shell time · {} runs",
            percent(row.share),
            count(row.runs)
        )
    } else {
        format!("not measured · {} runs", count(row.runs))
    }
}

pub fn time_note(row: &TimeRow) -> Option<String> {
    if row.capped_runs > 0.0 {
        return Some(format!(
            "{} of {} runs returned at a harness ceiling ({} of {}): these seconds are a floor",
            count(row.capped_runs),
            count(row.runs),
            hms(row.capped_sec),
            hms(row.total_sec)
        ));
    }
    (row.bg_runs > 0.0).then(|| {
        format!(
            "{} runs were backgrounded and charged only the seconds they took, which understates the wait they started",
            count(row.bg_runs)
        )
    })
}

/// Cause findings first, then fail rows with a detector, then the other
/// fail rows with a note, then time rows with a note.
pub fn findings_of(report: &StatsReport) -> Vec<Finding> {
    let causes = report.causes.iter().filter_map(|row| {
        let note = cause_note(row).filter(|_| row.hits > 0.0)?;
        Some(Finding {
            detector: Some(row.cause.as_str().to_owned()),
            impact: cause_impact(row),
            key: row.key.clone(),
            note,
            spread: cause_spread(row),
            title: row.label.clone(),
        })
    });
    let fails: Vec<Finding> = report
        .fails
        .iter()
        .filter_map(|row| {
            Some(Finding {
                detector: row.detector.clone(),
                impact: format!("{} sessions", count(row.sessions)),
                key: row.key.clone(),
                note: row.note.clone()?,
                spread: fail_spread(row),
                title: row.label.clone(),
            })
        })
        .collect();
    let times = report.time.iter().filter_map(|row| {
        Some(Finding {
            detector: None,
            impact: hms(row.total_sec),
            key: row.key.as_str().to_owned(),
            note: time_note(row)?,
            spread: time_spread(row, report.corpus.bash_sec),
            title: row.label.to_owned(),
        })
    });
    causes
        .chain(fails.iter().filter(|f| f.detector.is_some()).cloned())
        .chain(fails.iter().filter(|f| f.detector.is_none()).cloned())
        .chain(times)
        .collect()
}
