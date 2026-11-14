//! The stats screen's pure rules: number spelling, session chips, row
//! texts and bar widths, plus the text only core can write.

pub mod chips;
#[cfg(not(target_arch = "wasm32"))]
pub mod derive;
pub mod format;
pub mod rows;

use std::collections::HashMap;

use peektrace_protocol::{AgentId, DrillTable};

/// Which report: the window in days and an optional agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatsScope {
    pub agent: Option<AgentId>,
    pub days: u32,
}

impl Default for StatsScope {
    fn default() -> Self {
        Self {
            agent: None,
            days: rows::DEFAULT_DAYS,
        }
    }
}

/// One drill page to ask for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DrillAsk {
    pub table: DrillTable,
    pub key: String,
    pub offset: u32,
}

/// A row that names a change, as core words it in the inspector's format.
/// Its samples are the row's own, looked up in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingView {
    pub detector: Option<String>,
    pub impact: String,
    pub key: String,
    pub note: String,
    pub rule: Option<String>,
    pub spread: String,
    pub table: DrillTable,
    pub title: String,
}

/// The investigation prompts of one report, by row key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prompts {
    pub report: String,
    pub cause: HashMap<String, String>,
    pub fail: HashMap<String, String>,
    pub time: HashMap<String, String>,
}

impl Prompts {
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

/// Text core composes for a report: findings, rules, prompts and notes.
/// Native builds fill it; the web build has none and hides what needs it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsText {
    pub findings: Vec<FindingView>,
    pub suggested: Vec<String>,
    pub prompts: Prompts,
    /// Cause key to its rule check line.
    pub rule_lines: HashMap<String, String>,
    /// Cause key to the memory line it suggests.
    pub suggested_rule: HashMap<String, String>,
    /// Time key to its note.
    pub time_notes: HashMap<String, String>,
    /// `generatedAt` in local time.
    pub generated: String,
}

/// Calls within which a similar call counts as a retry (core `RETRY_WINDOW`).
pub const RETRY_WINDOW: f64 = 3.0;
