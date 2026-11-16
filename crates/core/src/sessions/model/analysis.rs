//! The analysis built on a parsed session: budget slices, snapshots,
//! on-disk context files and cost.

use super::ParsedSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKey {
    SystemTools,
    Listings,
    Memory,
    Files,
    Prompts,
    ToolResults,
    AssistantText,
    Thinking,
    Other,
    Unattributed,
}

impl BudgetKey {
    /// Budget display order.
    pub const ALL: [Self; 10] = [
        Self::SystemTools,
        Self::Listings,
        Self::Memory,
        Self::Files,
        Self::Prompts,
        Self::ToolResults,
        Self::AssistantText,
        Self::Thinking,
        Self::Other,
        Self::Unattributed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemTools => "system_tools",
            Self::Listings => "listings",
            Self::Memory => "memory",
            Self::Files => "files",
            Self::Prompts => "prompts",
            Self::ToolResults => "tool_results",
            Self::AssistantText => "assistant_text",
            Self::Thinking => "thinking",
            Self::Other => "other",
            Self::Unattributed => "unattributed",
        }
    }
}

/// Tokens per budget category.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BudgetSlices {
    pub system_tools: f64,
    pub listings: f64,
    pub memory: f64,
    pub files: f64,
    pub prompts: f64,
    pub tool_results: f64,
    pub assistant_text: f64,
    pub thinking: f64,
    pub other: f64,
    pub unattributed: f64,
}

impl BudgetSlices {
    pub fn get(&self, key: BudgetKey) -> f64 {
        match key {
            BudgetKey::SystemTools => self.system_tools,
            BudgetKey::Listings => self.listings,
            BudgetKey::Memory => self.memory,
            BudgetKey::Files => self.files,
            BudgetKey::Prompts => self.prompts,
            BudgetKey::ToolResults => self.tool_results,
            BudgetKey::AssistantText => self.assistant_text,
            BudgetKey::Thinking => self.thinking,
            BudgetKey::Other => self.other,
            BudgetKey::Unattributed => self.unattributed,
        }
    }
}

/// One budget row at the peak turn.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetSlice {
    pub key: BudgetKey,
    pub label: &'static str,
    pub short: &'static str,
    pub color: &'static str,
    pub estimated: bool,
    pub note: Option<&'static str>,
    pub tokens: f64,
}

/// Context make-up when one turn was sent.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnSnapshot {
    pub turn_index: usize,
    pub model: String,
    pub ctx: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub slices: BudgetSlices,
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextScope {
    Global,
    Project,
    Memory,
}

impl ContextScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Memory => "memory",
        }
    }
}

/// An instruction file Claude Code loads, as it sits on disk now.
#[derive(Debug, Clone, PartialEq)]
pub struct OnDiskContextFile {
    pub label: String,
    pub path: String,
    pub bytes: f64,
    pub tokens_est: f64,
    pub scope: ContextScope,
}

/// Session cost: the agent's own figure or an estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCost {
    /// True when the agent recorded it; else a list-price estimate.
    pub from_agent: bool,
    pub total_usd: Option<f64>,
    pub subagents_usd: Option<f64>,
    pub unpriced_models: Option<Vec<String>>,
    pub unreadable_subagents: Option<usize>,
}

/// A parsed session plus its context forensics. `parsed.turns` holds the
/// priced turns.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzedSession {
    pub parsed: ParsedSession,
    pub context_window: f64,
    pub context_window_inferred: bool,
    pub peak_context_tokens: f64,
    pub peak_turn_index: usize,
    pub final_context_tokens: f64,
    pub total_output_tokens: f64,
    pub system_overhead_tokens: f64,
    pub budget: Vec<BudgetSlice>,
    pub snapshots: Vec<TurnSnapshot>,
    pub on_disk_context_files: Vec<OnDiskContextFile>,
    pub dumb_zone_cross_turn: i64,
    pub dumb_zone_fraction: f64,
    pub dumb_zone_turns: usize,
    pub compaction_turns: Vec<usize>,
    pub turn_count: usize,
    pub user_message_count: usize,
    pub tool_call_count: usize,
    pub peak_cache_read_tokens: f64,
    pub web_search_requests: f64,
    pub web_fetch_requests: f64,
    /// `Some(NaN)` prints `null`, as `JSON.stringify` writes `NaN`.
    pub duration_ms: Option<f64>,
    pub cost: Option<SessionCost>,
}
