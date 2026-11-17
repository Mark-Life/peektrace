//! The stats text core writes for every surface: findings, prompts, rule
//! lines and time notes per report, plus the fixed section copy.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::DrillTable;
use crate::wire::stats::{CauseKind, SampleRef, WaitShape};

/// One finding: a row that names a change, and the route back to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The table it drills into; `None` for a cost finding.
    pub table: Option<DrillTable>,
    pub title: String,
    /// `Fix in: memory file`, and the other places a fix lives; `None` for a
    /// row with no named fix.
    pub fix_in: Option<String>,
    /// The sessions it came from.
    pub samples: Vec<SampleRef>,
}

/// The cells of the row that adds every cause up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CauseTotalsText {
    pub hits: String,
    pub retries: String,
    pub time: String,
    pub reread: String,
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StatsText {
    /// The one line the page opens with: what avoidable retries cost, and
    /// how much of them the shown fixes cover.
    pub verdict: Option<String>,
    /// What the verdict's total holds that is not a fixable failure.
    pub verdict_note: Option<String>,
    /// Rows that name a change, largest share of the spend first, then
    /// causes by hits since their last fix.
    pub findings: Vec<Finding>,
    /// Causes a memory rule already stopped.
    pub working: Vec<Finding>,
    /// Every suggested memory line, in the cause table's order.
    pub suggested: Vec<String>,
    pub prompts: StatsPrompts,
    /// Cause key to its rule check line.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub rule_lines: BTreeMap<String, String>,
    /// What the report covers, on one line: sessions, calls and the time
    /// every share divides by.
    pub corpus_line: String,
    /// The link to the measurement notes: `How these numbers are measured
    /// (10 notes) · 1 file skipped`.
    pub measured_link: String,
    /// Cause key to its retry-time cell: `32m14s · 2.8%`.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub cause_time: BTreeMap<String, String>,
    /// Cause key to its re-read cell: `17.4M · ~$9`.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub cause_reread: BTreeMap<String, String>,
    /// The row that adds every cause up; `None` with fewer than two causes.
    pub totals: Option<CauseTotalsText>,
    /// Cause key to every split of the row on one line.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub cause_split: BTreeMap<String, String>,
    /// Cause key to the row as one line, for a narrow screen.
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub cause_narrow: BTreeMap<String, String>,
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
    ranked_by: "largest share of the spend, then hits a fix would still save",
    note: None,
};

/// Findings a surface shows before the reader asks for more.
pub const FINDINGS_VISIBLE: usize = 3;

/// The collapsed list of causes a rule already stopped.
pub const WORKING_TITLE: &str = "Working";
pub const WORKING_NOTE: &str = "causes with no hit since their memory rule";

/// The link to the measurement notes; [`measured_link`] adds the count.
pub const MEASURED_TITLE: &str = "How these numbers are measured";

/// `How these numbers are measured (3 notes)`.
pub fn measured_link(notes: usize) -> String {
    let unit = if notes == 1 { "note" } else { "notes" };
    format!("{MEASURED_TITLE} ({notes} {unit})")
}

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

/// Who made a call: the reader's own sessions, or agents they started.
pub const YOU: &str = "You";
pub const SUBAGENTS: &str = "subagents";

/// `Fix in: memory file`, or `None` for a cause with no named fix.
pub fn fix_in(kind: CauseKind) -> Option<String> {
    let place = match kind {
        CauseKind::Rule => "memory file",
        CauseKind::Env => "your machine",
        CauseKind::Workflow => "workflow prompt",
        CauseKind::None => return None,
    };
    Some(format!("Fix in: {place}"))
}

/// [`fix_in`] for a kind spelled as on the wire: `rule`, `env`, `workflow`.
pub fn fix_in_of(kind: &str) -> Option<String> {
    fix_in(match kind {
        "rule" => CauseKind::Rule,
        "env" => CauseKind::Env,
        "workflow" => CauseKind::Workflow,
        _ => CauseKind::None,
    })
}

/// [`wait_shape_label`] for a shape spelled as on the wire: `loop-poll`.
pub fn wait_label_of(key: &str) -> Option<&'static str> {
    let shape = match key {
        "loop-poll" => WaitShape::LoopPoll,
        "watcher" => WaitShape::Watcher,
        "curl-retry" => WaitShape::CurlRetry,
        "bare-delay" => WaitShape::BareDelay,
        _ => return None,
    };
    Some(wait_shape_label(shape))
}

/// How a waiting call parks, in plain words.
pub const fn wait_shape_label(shape: WaitShape) -> &'static str {
    match shape {
        WaitShape::LoopPoll => "polling loop",
        WaitShape::Watcher => "watcher",
        WaitShape::CurlRetry => "curl retry",
        WaitShape::BareDelay => "fixed sleep",
    }
}

/// The collapsed tables under the findings.
pub const DATA_TITLE: &str = "Show the data";
pub const DATA_HIDE: &str = "Hide the data";

/// One tab of the data section: its title, and the line under it that says
/// what the tab ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataTab {
    pub title: &'static str,
    pub note: &'static str,
}

pub const RETRIES_TAB: DataTab = DataTab {
    title: "Retries",
    note: "Every named cause, ranked by sessions with a retry. A retry is a failed call followed by a similar call of the same tool that succeeded.",
};
pub const FAILS_TAB: DataTab = DataTab {
    title: "Tool errors",
    note: "Calls whose output printed a failure, ranked by sessions spanned. A row never says that something broke.",
};
pub const TIME_TAB: DataTab = DataTab {
    title: "Time",
    note: "Shell time by job. Each command line is charged to exactly one job. Tools that wait on a human are left out.",
};

/// The row under the cause table that adds every cause up.
pub const TOTALS_LABEL: &str = "All causes";

/// A column heading and the definition its tooltip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub label: &'static str,
    pub tip: &'static str,
}

pub const COL_HITS: Column = Column {
    label: "Hits",
    tip: "Calls that failed with this cause.",
};
pub const COL_RETRIES: Column = Column {
    label: "Retries",
    tip: "Failed calls followed by a similar call that succeeded: a round trip a fix would save. Read-backs of large output are not failures.",
};
pub const COL_SPLIT: Column = Column {
    label: "You / subagents",
    tip: "Hits in your own sessions, then in subagents.",
};
pub const COL_RETRY_TIME: Column = Column {
    label: "Retry time",
    tip: "Time from the failed call to the retry, and its share of shell time. Causes outside the shell, and the total, are a share of all tool time.",
};
pub const COL_REREAD: Column = Column {
    label: "Re-read",
    tip: "Context the retrying model call read again, priced at each session's model. The dollar figure is approximate.",
};
pub const COL_RULE: Column = Column {
    label: "Rule",
    tip: "A memory-file line that states this fix, and whether the cause still fires since.",
};
pub const COL_FLAGGED: Column = Column {
    label: "Flagged",
    tip: "Calls that set the agent's own error flag. The gap to Calls is what an error panel alone would miss.",
};
pub const COL_SHARE: Column = Column {
    label: "Share",
    tip: "Share of all shell time.",
};
pub const COL_OTHER_JOBS: Column = Column {
    label: "In other jobs",
    tip: "Command lines that also run this job but are charged to a higher-priority one.",
};

/// The metadata line at the foot of the page.
pub const FOOTER_REDACTED: &str =
    "Secrets such as API keys and tokens are masked. Commands and paths are shown as run.";
/// The switch that shows detector ids and fix kinds.
pub const DEBUG_TOGGLE: &str = "Show detector ids";
