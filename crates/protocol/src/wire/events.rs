//! Timeline events, as whole events and as history index rows, and the
//! pages that carry them.

use serde::{Deserialize, Serialize};

use crate::view::{EventKind, PromptOrigin};
use crate::{HistoryIndexMeta, HistoryPageMeta};

/// Where an image's bytes sit: content block `block` of the event's line,
/// and item `item` inside it when that block is a tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub block: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<usize>,
    pub media_type: String,
}

/// Budget category an attachment adds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum LoadedCategory {
    ClaudeMd,
    Skills,
    Agents,
    Tools,
    Mcp,
    File,
    Memory,
    Ide,
    Reminder,
    Other,
}

/// One earlier edit of an entry edited more than once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ContextEditStep {
    pub at_index: f64,
    pub tokens_est: f64,
    pub thinking_est: f64,
}

/// A later entry (at line `atIndex`) changed this event's share of the
/// context. History index rows leave out `original`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ContextEdit {
    #[cfg_attr(test, ts(optional_fields))]
    #[serde(rename_all = "camelCase")]
    Removed {
        at_index: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        steps: Option<Vec<ContextEditStep>>,
    },
    #[cfg_attr(test, ts(optional_fields))]
    #[serde(rename_all = "camelCase")]
    Replaced {
        at_index: f64,
        original_tokens_est: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        original: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        steps: Option<Vec<ContextEditStep>>,
    },
}

/// The keys an event and its history index row share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct EventFields {
    pub index: usize,
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_sidechain: Option<bool>,
    /// The harness wrote this line, not the person at the keyboard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_meta: Option<bool>,
    /// Who wrote a `user-prompt`; absent when the transcript does not say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_origin: Option<PromptOrigin>,
    /// Claude Code's own `promptSource` for the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_source: Option<String>,
    pub title: String,
    pub tokens_est: f64,
    /// Growth in prompt size between the model calls around this event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_measured: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// A server tool's call or result (web search, web fetch, advisor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_tool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loaded_category: Option<LoadedCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_edit: Option<ContextEdit>,
    /// Context size before a compaction (`compact_boundary`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact_pre_tokens: Option<f64>,
    /// Context size right after a compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact_post_tokens: Option<f64>,
}

/// One entry of the session timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    #[serde(flatten)]
    pub fields: EventFields,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub preview: String,
    pub body: String,
}

/// `GET /api/sessions/{id}/history`: one page of whole events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryPage {
    #[serde(flatten)]
    pub meta: HistoryPageMeta,
    pub events: Vec<TimelineEvent>,
}

/// One history index row: an event without its text, timestamp and request
/// id, plus its turn, body length (UTF-16) and the first position with the
/// same body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    #[serde(flatten)]
    pub fields: EventFields,
    pub turn: usize,
    pub body_len: usize,
    pub body_id: usize,
}

/// `GET /api/sessions/{id}/history/index`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryIndex {
    #[serde(flatten)]
    pub meta: HistoryIndexMeta,
    pub rows: Vec<HistoryRow>,
}
