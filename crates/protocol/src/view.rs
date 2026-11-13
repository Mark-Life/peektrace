//! Typed read-side shapes of the session list and the session analysis, as
//! the core JSON writers print them. Only the fields the app reads are typed;
//! the rest are skipped, so new fields pass.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{AgentId, UnreadableSession};

/// A JSON number, or `null` (how `JSON.stringify` prints `NaN`) as `NaN`.
fn num<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Option::<f64>::deserialize(d).map(|n| n.unwrap_or(f64::NAN))
}

fn opt_num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(d).map(|n| Some(n.unwrap_or(f64::NAN)))
}

/// One row of `GET /api/sessions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub id: String,
    pub agent: AgentId,
    pub path: String,
    pub project: String,
    pub message_count: u64,
    pub size_bytes: u64,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
}

/// `GET /api/sessions`, typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionListingView {
    pub headers: Vec<SessionHeader>,
    pub unreadable: Vec<UnreadableSession>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    UserPrompt,
    AssistantText,
    AssistantThinking,
    ToolCall,
    ToolResult,
    Attachment,
    SystemPrompt,
    System,
    Compaction,
    Summary,
    Meta,
}

impl EventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserPrompt => "user-prompt",
            Self::AssistantText => "assistant-text",
            Self::AssistantThinking => "assistant-thinking",
            Self::ToolCall => "tool-call",
            Self::ToolResult => "tool-result",
            Self::Attachment => "attachment",
            Self::SystemPrompt => "system-prompt",
            Self::System => "system",
            Self::Compaction => "compaction",
            Self::Summary => "summary",
            Self::Meta => "meta",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptOrigin {
    Human,
    Sdk,
    Command,
    TaskNotification,
    Scheduled,
    Harness,
}

impl PromptOrigin {
    /// Sent by a person: typed, through the SDK, or a slash command.
    pub const fn from_user(self) -> bool {
        matches!(self, Self::Human | Self::Sdk | Self::Command)
    }
}

/// One row of the session timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub index: usize,
    pub kind: EventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    pub title: String,
    pub preview: String,
    pub body: String,
    #[serde(deserialize_with = "num")]
    pub tokens_est: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub tokens_measured: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_sidechain: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_meta: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_origin: Option<PromptOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_source: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub thinking_duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_tool: Option<bool>,
    /// Image refs; this view reads only whether there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_edit: Option<ContextEdit>,
}

impl TimelineEvent {
    pub fn sidechain(&self) -> bool {
        self.is_sidechain == Some(true)
    }

    /// A user prompt a person wrote; unclassified prompts count as theirs.
    pub fn is_user_authored(&self) -> bool {
        self.kind == EventKind::UserPrompt && self.prompt_origin.is_none_or(PromptOrigin::from_user)
    }
}

/// A later change to an event's place in model context; only its status is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEdit {
    pub status: String,
}

/// One model call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub request_id: String,
    pub model: String,
    #[serde(deserialize_with = "num")]
    pub context_tokens: f64,
    #[serde(deserialize_with = "num")]
    pub output_tokens: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub thinking_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub web_search_requests: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub web_fetch_requests: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub turns: u64,
    #[serde(deserialize_with = "num")]
    pub peak_context_tokens: f64,
}

/// A budget category at the peak turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetSlice {
    pub key: String,
    pub label: String,
    pub short: String,
    pub color: String,
    pub estimated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(deserialize_with = "num")]
    pub tokens: f64,
}

/// Tokens per budget category, in stack order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BudgetSlices {
    #[serde(default, deserialize_with = "num")]
    pub system_tools: f64,
    #[serde(default, deserialize_with = "num")]
    pub listings: f64,
    #[serde(default, deserialize_with = "num")]
    pub memory: f64,
    #[serde(default, deserialize_with = "num")]
    pub files: f64,
    #[serde(default, deserialize_with = "num")]
    pub prompts: f64,
    #[serde(default, deserialize_with = "num")]
    pub tool_results: f64,
    #[serde(default, deserialize_with = "num")]
    pub assistant_text: f64,
    #[serde(default, deserialize_with = "num")]
    pub thinking: f64,
    #[serde(default, deserialize_with = "num")]
    pub other: f64,
    #[serde(default, deserialize_with = "num")]
    pub unattributed: f64,
}

impl BudgetSlices {
    /// Floor to top stacking order of the budget keys.
    pub const STACK_ORDER: [&'static str; 10] = [
        "system_tools",
        "listings",
        "memory",
        "files",
        "prompts",
        "tool_results",
        "assistant_text",
        "thinking",
        "other",
        "unattributed",
    ];

    /// Values in [`Self::STACK_ORDER`].
    pub const fn stacked(&self) -> [f64; 10] {
        [
            self.system_tools,
            self.listings,
            self.memory,
            self.files,
            self.prompts,
            self.tool_results,
            self.assistant_text,
            self.thinking,
            self.other,
            self.unattributed,
        ]
    }
}

/// Context make-up when one turn was sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSnapshot {
    pub turn_index: usize,
    pub model: String,
    #[serde(deserialize_with = "num")]
    pub ctx: f64,
    #[serde(deserialize_with = "num")]
    pub output_tokens: f64,
    #[serde(deserialize_with = "num")]
    pub cache_read_tokens: f64,
    pub slices: BudgetSlices,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostSource {
    Agent,
    Estimate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCost {
    pub source: CostSource,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub total_usd: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_num"
    )]
    pub subagents_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpriced_models: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable_subagents: Option<u64>,
}

/// `GET /api/sessions/{id}/analysis`, typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedSession {
    pub provider: String,
    pub session_id: String,
    pub path: String,
    pub models: Vec<String>,
    pub events: Vec<TimelineEvent>,
    pub turns: Vec<Turn>,
    pub compaction_indexes: Vec<usize>,
    pub subagents: Vec<SubagentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(deserialize_with = "num")]
    pub context_window: f64,
    pub context_window_inferred: bool,
    #[serde(deserialize_with = "num")]
    pub peak_context_tokens: f64,
    pub peak_turn_index: usize,
    #[serde(deserialize_with = "num")]
    pub total_output_tokens: f64,
    #[serde(deserialize_with = "num")]
    pub system_overhead_tokens: f64,
    pub budget: Vec<BudgetSlice>,
    pub snapshots: Vec<TurnSnapshot>,
    pub dumb_zone_cross_turn: i64,
    #[serde(deserialize_with = "num")]
    pub dumb_zone_fraction: f64,
    pub dumb_zone_turns: u64,
    pub compaction_turns: Vec<usize>,
    pub turn_count: u64,
    pub user_message_count: u64,
    pub tool_call_count: u64,
    #[serde(deserialize_with = "num")]
    pub peak_cache_read_tokens: f64,
    #[serde(deserialize_with = "num")]
    pub web_search_requests: f64,
    #[serde(deserialize_with = "num")]
    pub web_fetch_requests: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<SessionCost>,
}

/// Replace every `\uD800`-`\uDFFF` escape that is not half of a valid pair
/// with `�`, so `serde_json` (which rejects lone surrogates) reads the
/// text the way a browser shows it. Bytes outside JSON strings never hold
/// `\u`, so a plain scan is enough.
pub fn mend_surrogates(body: &str) -> std::borrow::Cow<'_, str> {
    let b = body.as_bytes();
    let hex = |i: usize| -> Option<u16> {
        let s = b.get(i..i + 4)?;
        u16::from_str_radix(std::str::from_utf8(s).ok()?, 16).ok()
    };
    let esc = |i: usize| -> Option<u16> {
        (b.get(i) == Some(&b'\\') && b.get(i + 1) == Some(&b'u'))
            .then(|| hex(i + 2))
            .flatten()
    };
    let mut out: Option<String> = None;
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        let Some(u) = esc(i) else {
            // `\\` and other escapes: skip both bytes so `\\u` is not read as `\u`.
            i += 2;
            continue;
        };
        if (0xD800..0xDC00).contains(&u)
            && esc(i + 6).is_some_and(|l| (0xDC00..0xE000).contains(&l))
        {
            i += 12;
            continue;
        }
        if (0xD800..0xE000).contains(&u) {
            let o = out.get_or_insert_with(|| String::with_capacity(body.len()));
            o.push_str(&body[last..i]);
            o.push_str("\\uFFFD");
            last = i + 6;
        }
        i += 6;
    }
    match out {
        None => std::borrow::Cow::Borrowed(body),
        Some(mut o) => {
            o.push_str(&body[last..]);
            std::borrow::Cow::Owned(o)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mends_lone_surrogates_only() {
        // `u` builds `\\uXXXX` escapes, so the source holds no literal ones.
        let u = |hex: &str| format!("{}u{hex}", '\\');
        let q = |s: String| format!("\"{s}\"");
        let fffd = u("FFFD");
        assert_eq!(
            mend_surrogates(&q(format!("a{}b", u("d800")))),
            q(format!("a{fffd}b"))
        );
        assert_eq!(mend_surrogates(&q(u("dc00"))), q(fffd.clone()));
        let pair = format!("{}{}", u("d83d"), u("de00"));
        assert_eq!(mend_surrogates(&q(pair.clone())), q(pair));
        let escaped = format!("{}{}", '\\', u("d800"));
        assert_eq!(mend_surrogates(&q(escaped.clone())), q(escaped));
        let two = format!("{}{}", u("d800"), u("d800"));
        assert_eq!(mend_surrogates(&q(two)), q(format!("{fffd}{fffd}")));
        let s: String =
            serde_json::from_str(&mend_surrogates(&q(format!("x{}y", u("dbff"))))).unwrap();
        assert_eq!(s, "x\u{FFFD}y");
    }

    #[test]
    fn reads_nan_as_null() {
        let s: BudgetSlices = serde_json::from_str(r#"{"files":null,"other":2}"#).unwrap();
        assert!(s.files.is_nan());
        assert!((s.other - 2.0).abs() < f64::EPSILON);
    }
}
