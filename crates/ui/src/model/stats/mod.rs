//! The stats screen's pure rules: number spelling, session chips, row
//! texts and bar widths, plus the text only core can write.

pub mod chips;
#[cfg(not(target_arch = "wasm32"))]
pub mod derive;
pub mod format;
pub mod rows;

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

/// Core's text for a report: findings, rules, prompts and notes. The web
/// build reads it from the API; the desktop app builds it in process.
pub use peektrace_protocol::stats_text::{
    Finding as FindingView, StatsPrompts as Prompts, StatsText,
};

/// Calls within which a similar call counts as a retry (core `RETRY_WINDOW`).
pub const RETRY_WINDOW: f64 = 3.0;
