//! Session list rows, parsed sessions and analyses.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::events::TimelineEvent;
use crate::{AgentId, SummaryHistory};

/// The agent that wrote a transcript, as the parsed session names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    ClaudeCode,
    Codex,
    Pi,
    Opencode,
}

/// One model call, with its usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    pub model: String,
    pub context_tokens: f64,
    pub input_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    /// Cache writes with a 1-hour TTL; absent on old transcripts.
    #[serde(
        rename = "cacheCreation1hTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_creation_1h_tokens: Option<f64>,
    /// Cache writes with a 5-minute TTL.
    #[serde(
        rename = "cacheCreation5mTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_creation_5m_tokens: Option<f64>,
    pub output_tokens: f64,
    /// The thinking share of `outputTokens`, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_tokens: Option<f64>,
    /// Reasoning effort the harness asked for (`low` … `max`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_requests: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_fetch_requests: Option<f64>,
    /// USD at list price, or the agent's own figure; absent with no rate card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Empty for a billed call on an abandoned branch.
    pub event_indexes: Vec<usize>,
}

/// The cost of a list of turns at list price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct TurnsCost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Model ids with no rate card; set only when non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpriced_models: Option<Vec<String>>,
}

/// A spawned subagent transcript found on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SubagentRef {
    #[serde(flatten)]
    pub cost: TurnsCost,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The call that launched this agent; in the parent subagent's
    /// transcript when `parentAgentId` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub path: String,
    pub turns: f64,
    pub peak_context_tokens: f64,
    /// The `wf_*` workflow run this agent belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_phase_index: Option<f64>,
    /// The subagent that spawned this one, as `agent-<id>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    /// The transcript could not be read; `turns` and the peak are then 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "true"))]
    pub unreadable: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ContextFileScope {
    Global,
    Project,
    Memory,
}

/// A CLAUDE.md, rules or MEMORY.md file read from disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct OnDiskContextFile {
    pub label: String,
    pub path: String,
    pub bytes: f64,
    pub tokens_est: f64,
    pub scope: ContextFileScope,
}

/// Context-budget categories, floor to top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
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

/// Tokens per budget category; with `unattributed` they sum to the turn's context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

/// One slice of the budget at the peak turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
pub struct BudgetSlice {
    pub key: BudgetKey,
    pub label: String,
    pub short: String,
    pub tokens: f64,
    pub color: String,
    pub estimated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Context make-up when one turn was sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct TurnSnapshot {
    pub turn_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    pub model: String,
    pub ctx: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub slices: BudgetSlices,
}

/// The app that started a session (e.g. t3code), with the branch and
/// worktree it gave the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Launcher {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
}

/// A parsed session without its events and turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ParsedMeta {
    pub provider: Provider,
    pub session_id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub compaction_indexes: Vec<usize>,
    pub subagents: Vec<SubagentRef>,
    /// Context-window size the transcript states; preferred over the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_context_window: Option<f64>,
    /// Record types the parser does not know, with line counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "Record<string, number>"))]
    pub unknown_record_types: Option<BTreeMap<String, f64>>,
    /// `system` subtypes the parser does not know, with line counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "Record<string, number>"))]
    pub unknown_system_subtypes: Option<BTreeMap<String, f64>>,
    /// USD the agent itself recorded (Claude Code, Pi, `OpenCode`); set only
    /// above 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_cost_usd: Option<f64>,
    /// True when `nativeCostUsd` already counts subagents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_cost_includes_subagents: Option<bool>,
    /// True when `nativeCostUsd` misses part of the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_cost_partial: Option<bool>,
    /// Cost of inline sidechain turns, which `turns` leaves out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidechain_cost: Option<TurnsCost>,
    /// The app that started the session, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launcher: Option<Launcher>,
}

/// `GET /api/sessions/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ParsedSession {
    #[serde(flatten)]
    pub meta: ParsedMeta,
    pub events: Vec<TimelineEvent>,
    pub turns: Vec<Turn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum CostSource {
    Estimate,
    Agent,
}

/// A session's cost: an estimate at list price, or the agent's own figure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SessionCost {
    /// Main transcript plus every subagent; absent when nothing is priced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_usd: Option<f64>,
    /// Estimated subagent cost, also when `totalUsd` is recorded; set only
    /// above 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents_usd: Option<f64>,
    /// Where `totalUsd` comes from: the agent's own record or our estimate.
    pub source: CostSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpriced_models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable_subagents: Option<f64>,
    /// The agent's own figure, also when it covers only part of the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_usd: Option<f64>,
    /// Our token estimate over what `recordedUsd` covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_usd: Option<f64>,
    /// True when `recordedUsd` misses part of the session, so the total is
    /// the estimate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_partial: Option<bool>,
    /// `(estimateUsd - recordedUsd) / recordedUsd`, set only past 5% when the
    /// record is the total: a model may lack a price card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_gap: Option<f64>,
    /// True when the record is the total and the estimate points at a price
    /// problem: a model is unpriced, the estimate is more than 5% above the
    /// record, or more than 35% below it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_warning: Option<bool>,
}

/// The analysis without its events, turns and snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisMeta {
    #[serde(flatten)]
    pub parsed: ParsedMeta,
    pub context_window: f64,
    pub context_window_inferred: bool,
    pub peak_context_tokens: f64,
    pub peak_turn_index: usize,
    pub final_context_tokens: f64,
    pub total_output_tokens: f64,
    pub system_overhead_tokens: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    pub budget: Vec<BudgetSlice>,
    pub on_disk_context_files: Vec<OnDiskContextFile>,
    /// First turn at or past the dumb zone, or -1.
    pub dumb_zone_cross_turn: f64,
    pub dumb_zone_fraction: f64,
    pub dumb_zone_turns: f64,
    pub compaction_turns: Vec<usize>,
    pub turn_count: f64,
    pub user_message_count: f64,
    pub tool_call_count: f64,
    pub peak_cache_read_tokens: f64,
    pub web_search_requests: f64,
    pub web_fetch_requests: f64,
    /// Absent when the session has no turns to price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<SessionCost>,
}

/// `GET /api/sessions/{id}/analysis`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnalyzedSession {
    #[serde(flatten)]
    pub meta: AnalysisMeta,
    pub events: Vec<TimelineEvent>,
    pub turns: Vec<Turn>,
    pub snapshots: Vec<TurnSnapshot>,
}

/// `GET /api/sessions/{id}/summary`: the analysis without its arrays, plus
/// the parts' hashes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionSummary {
    #[serde(flatten)]
    pub meta: AnalysisMeta,
    pub history: SummaryHistory,
}

/// One row of `GET /api/sessions`: read from the transcript head, no full parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub id: String,
    pub path: String,
    pub agent: AgentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub updated_at: String,
    pub message_count: f64,
    pub size_bytes: f64,
    /// Source id when the agent has more than one configured root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Label of that source, shown as a badge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    /// Malformed lines other than a partial last line; set only above 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub malformed_lines: Option<f64>,
    /// The app that started the session, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launcher: Option<Launcher>,
    /// Cost of the main transcript, inline sidechains included.
    #[serde(flatten)]
    pub cost: TurnsCost,
}
