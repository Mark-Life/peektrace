//! The stats text core writes for every surface: findings, prompts, rule
//! lines and time notes per report, plus the fixed section copy.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::DrillTable;

/// One finding: a row that names a change, and the route back to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub detector: Option<String>,
    /// What it costs, in the unit its own table ranks by.
    pub impact: String,
    pub key: String,
    pub note: String,
    /// The memory line this finding suggests copying.
    pub rule: Option<String>,
    /// How wide it reaches, in that table's own columns.
    pub spread: String,
    pub table: DrillTable,
    pub title: String,
}

/// The investigation prompts of one report, by row key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StatsPrompts {
    pub report: String,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub cause: BTreeMap<String, String>,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub fail: BTreeMap<String, String>,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub time: BTreeMap<String, String>,
}

impl StatsPrompts {
    pub fn of(&self, table: DrillTable, key: &str) -> Option<&str> {
        match table {
            DrillTable::Cause => self.cause.get(key),
            DrillTable::Fail => self.fail.get(key),
            DrillTable::Time => self.time.get(key),
            DrillTable::Cluster | DrillTable::Wait => None,
        }
        .map(String::as_str)
    }
}

/// Text core composes for one report, in the inspector's number format.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StatsText {
    /// Rows that name a change, cheapest change first.
    pub findings: Vec<Finding>,
    /// Every suggested memory line, in the cause table's order.
    pub suggested: Vec<String>,
    pub prompts: StatsPrompts,
    /// Cause key to its rule check line.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub rule_lines: BTreeMap<String, String>,
    /// Cause key to the memory line it suggests.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub suggested_rule: BTreeMap<String, String>,
    /// Time key to its note.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub time_notes: BTreeMap<String, String>,
}

/// One stats section: its heading, what it ranks by, and its lead note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub ranked_by: &'static str,
    pub note: Option<&'static str>,
}

pub const FINDINGS_SECTION: Section = Section {
    title: "Flagged findings",
    ranked_by: "cheapest fix first, then by each table's own impact",
    note: None,
};

/// The retry note names the retry window, so [`retry_note`] writes it.
pub const CAUSES_SECTION: Section = Section {
    title: "Avoidable retries",
    ranked_by: "sessions with a retry, then sessions, then retries",
    note: None,
};

pub const FAILS_SECTION: Section = Section {
    title: "What fails",
    ranked_by: "sessions spanned, then calls",
    note: Some(
        "A row says a call printed a failure — never that something broke. Flagged counts the calls that set the agent's own error flag; the gap between it and Calls is what an error panel alone would miss.",
    ),
};

pub const TIME_SECTION: Section = Section {
    title: "What costs time",
    ranked_by: "seconds charged",
    note: Some(
        "Each command line is charged to exactly one job; “In other jobs” counts the lines that also run the job but are charged to a higher-priority one. Tools that wait on a human are out of the denominator.",
    ),
};

pub const WAITING_SECTION: Section = Section {
    title: "Waiting",
    ranked_by: "seconds parked",
    note: None,
};

pub const CLUSTERS_SECTION: Section = Section {
    title: "Error clusters",
    ranked_by: "rows, then sessions spanned",
    note: None,
};

/// The lead note of the retries section; `window` is the retry window.
pub fn retry_note(window: &str) -> String {
    format!(
        "A retry is a failed call followed within {window} calls by a similar call of the same tool that succeeded: a round trip a rule or an env fix would have saved. Retry time is the call-to-call gap; context re-read is what the retrying model call carried. Neither is added to the other."
    )
}

/// The suggested-rules box under the findings.
pub const SUGGESTED_TITLE: &str = "Suggested rules";
pub const SUGGESTED_NOTE: &str = "lines no CLAUDE.md / AGENTS.md states yet";

pub const EMPTY_FINDINGS: &str =
    "No row in this window carries a named fix. The tables below still rank what happened.";
pub const EMPTY_CAUSES: &str = "No named cause fired in this window.";
pub const EMPTY_FAILS: &str = "Nothing failed in this window.";
pub const EMPTY_TIME: &str = "No shell time measured in this window.";
