//! The parsed-session model every agent parser produces, and the analysis
//! built on it. Strings are transcript text (see [`crate::js::text::units`]);
//! counts read from a transcript are `f64`, as in JS.
//!
//! JSON key order differs by producer, so events and turns carry a shape tag
//! that fixes it (see `write.rs`). A parser for another agent adds its own
//! shapes; the structs stay the same.

/// Which agent wrote the transcript, as the JSON `provider` field names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    ClaudeCode,
    Codex,
    Pi,
    Opencode,
}

impl Provider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Model output: text, thinking or a tool call.
    pub const fn is_output(self) -> bool {
        matches!(
            self,
            Self::AssistantText | Self::AssistantThinking | Self::ToolCall
        )
    }
}

/// Who wrote a user prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOrigin {
    Human,
    Sdk,
    Command,
    TaskNotification,
    Scheduled,
    Harness,
}

impl PromptOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Sdk => "sdk",
            Self::Command => "command",
            Self::TaskNotification => "task-notification",
            Self::Scheduled => "scheduled",
            Self::Harness => "harness",
        }
    }

    /// Sent by a person: typed, through the SDK, or a slash command.
    pub const fn from_user(self) -> bool {
        matches!(self, Self::Human | Self::Sdk | Self::Command)
    }
}

/// The budget category an attachment loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl LoadedCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeMd => "claude-md",
            Self::Skills => "skills",
            Self::Agents => "agents",
            Self::Tools => "tools",
            Self::Mcp => "mcp",
            Self::File => "file",
            Self::Memory => "memory",
            Self::Ide => "ide",
            Self::Reminder => "reminder",
            Self::Other => "other",
        }
    }
}

/// Where an image sits in a Claude user line.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    pub block: usize,
    pub media_type: String,
    pub item: Option<usize>,
}

/// One earlier edit of an event's context size.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextEditStep {
    pub at_index: f64,
    pub tokens_est: f64,
    pub thinking_est: f64,
}

/// A later change to an event's place in model context (Pi).
#[derive(Debug, Clone, PartialEq)]
pub enum ContextEdit {
    Removed {
        at_index: f64,
        steps: Option<Vec<ContextEditStep>>,
    },
    Replaced {
        at_index: f64,
        original_tokens_est: f64,
        original: Option<String>,
        steps: Option<Vec<ContextEditStep>>,
    },
}

impl ContextEdit {
    pub fn at_index(&self) -> f64 {
        match self {
            Self::Removed { at_index, .. } | Self::Replaced { at_index, .. } => *at_index,
        }
    }

    pub fn steps(&self) -> &[ContextEditStep] {
        match self {
            Self::Removed { steps, .. } | Self::Replaced { steps, .. } => {
                steps.as_deref().unwrap_or_default()
            }
        }
    }

    pub const fn status(&self) -> &'static str {
        match self {
            Self::Removed { .. } => "removed",
            Self::Replaced { .. } => "replaced",
        }
    }
}

/// Which producer built an event; fixes its JSON key order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventShape {
    /// Claude user `tool_result` block.
    ToolResult,
    /// A user prompt.
    Prompt,
    /// A queued prompt: a prompt plus its attachment type.
    QueuedPrompt,
    /// Claude user image block.
    Image,
    /// Claude compaction summary.
    Compaction,
    /// Any block of a Claude assistant message.
    AssistantBlock,
    /// Claude rolling summary.
    Summary,
    /// Claude `system` line.
    System,
    /// Claude `attachment` line.
    Attachment,
    /// The shared system-prompt builder.
    SystemPrompt,
}

/// One row of the session timeline. Fields a producer does not set are
/// `None` and print nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEvent {
    pub shape: EventShape,
    /// Row position in the parsed JSONL rows.
    pub index: usize,
    pub kind: EventKind,
    pub ts: Option<String>,
    pub request_id: Option<String>,
    pub is_sidechain: Option<bool>,
    pub is_meta: Option<bool>,
    pub prompt_origin: Option<PromptOrigin>,
    pub prompt_source: Option<String>,
    pub title: String,
    pub preview: String,
    pub body: String,
    pub tokens_est: f64,
    /// Always printed last: every producer adds it after the fact.
    pub tokens_measured: Option<f64>,
    pub is_error: Option<bool>,
    pub server_tool: Option<bool>,
    pub thinking_duration_ms: Option<f64>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub attachment_type: Option<String>,
    pub images: Option<Vec<ImageRef>>,
    pub loaded_category: Option<LoadedCategory>,
    pub context_edit: Option<ContextEdit>,
    pub compact_pre_tokens: Option<f64>,
    pub compact_post_tokens: Option<f64>,
}

impl TimelineEvent {
    /// An event with the required fields set and every optional one unset.
    pub fn new(shape: EventShape, index: usize, kind: EventKind) -> Self {
        Self {
            shape,
            index,
            kind,
            ts: None,
            request_id: None,
            is_sidechain: None,
            is_meta: None,
            prompt_origin: None,
            prompt_source: None,
            title: String::new(),
            preview: String::new(),
            body: String::new(),
            tokens_est: 0.0,
            tokens_measured: None,
            is_error: None,
            server_tool: None,
            thinking_duration_ms: None,
            tool_name: None,
            tool_use_id: None,
            attachment_type: None,
            images: None,
            loaded_category: None,
            context_edit: None,
            compact_pre_tokens: None,
            compact_post_tokens: None,
        }
    }

    /// Set title, preview, body and the token estimate in one go.
    #[must_use]
    pub fn text(mut self, title: String, preview: String, body: String, tokens_est: f64) -> Self {
        self.title = title;
        self.preview = preview;
        self.body = body;
        self.tokens_est = tokens_est;
        self
    }

    pub fn sidechain(&self) -> bool {
        self.is_sidechain == Some(true)
    }

    /// A user prompt the user (not the harness) wrote. Unclassified prompts
    /// count as the user's.
    pub fn is_user_authored(&self) -> bool {
        self.kind == EventKind::UserPrompt && self.prompt_origin.is_none_or(PromptOrigin::from_user)
    }
}

/// Turn fields a later line can add after the turn was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnField {
    ThinkingTokens,
    WebSearchRequests,
    WebFetchRequests,
    CostUsd,
}

/// Which producer built a turn; fixes its JSON key order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnShape {
    Claude,
}

/// One model call.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub shape: TurnShape,
    pub request_id: String,
    pub model: String,
    pub context_tokens: f64,
    pub input_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub output_tokens: f64,
    pub cache_creation_1h_tokens: Option<f64>,
    pub cache_creation_5m_tokens: Option<f64>,
    /// Positions in `events`; `-1` when a block came before any event.
    pub event_indexes: Vec<i64>,
    pub ts: Option<String>,
    pub thinking_tokens: Option<f64>,
    pub web_search_requests: Option<f64>,
    pub web_fetch_requests: Option<f64>,
    pub effort: Option<String>,
    pub cost_usd: Option<f64>,
    /// Keys set after the turn was built, in the order they were added.
    pub appended: Vec<TurnField>,
}

/// Cost of a list of turns.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnsCost {
    pub cost_usd: Option<f64>,
    pub unpriced_models: Option<Vec<String>>,
}

/// A subagent transcript folded into its parent session.
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentRef {
    pub id: String,
    pub path: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub tool_use_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub workflow_run_id: Option<String>,
    pub workflow_phase: Option<String>,
    pub workflow_phase_index: Option<f64>,
    /// `tool_use_id` came from the workflow link, so it prints after the
    /// workflow keys.
    pub tool_use_id_linked: bool,
    pub turns: usize,
    pub peak_context_tokens: f64,
    pub cost: TurnsCost,
    pub unreadable: bool,
}

/// One parsed transcript. Optional fields print only when set; `""` counts
/// as set.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSession {
    pub provider: Provider,
    pub session_id: String,
    pub path: String,
    pub models: Vec<String>,
    pub events: Vec<TimelineEvent>,
    pub turns: Vec<Turn>,
    pub compaction_indexes: Vec<usize>,
    pub subagents: Vec<SubagentRef>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub title: Option<String>,
    pub version: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub native_context_window: Option<f64>,
    /// Counts in JS key order.
    pub unknown_record_types: Option<Vec<(String, f64)>>,
    pub unknown_system_subtypes: Option<Vec<(String, f64)>>,
    pub sidechain_cost: Option<TurnsCost>,
    pub native_cost_usd: Option<f64>,
}

mod analysis;

pub use analysis::*;
