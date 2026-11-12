//! The display-ready export shape shared by the Markdown, HTML and JSON
//! renderers: a field whitelist, home paths collapsed when redacted.

use super::home::HomeCollapser;
use crate::js::units;
use crate::sessions::labels::{context_edit_note, event_badge_label, thinking_note};
use crate::sessions::model::{
    ContextEdit, EventKind, ParsedSession, PromptOrigin, Provider, TimelineEvent,
};

pub const LABEL_MAX: usize = 80;
pub const TITLE_MAX: usize = 200;
pub const NOTE_MAX: usize = 400;
const LINE_MAX: usize = 120;

/// How a row renders: chat for `user`/`assistant`, collapsed for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Other,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Other => "other",
        }
    }
}

/// One exported timeline row.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportEvent {
    pub index: usize,
    pub kind: EventKind,
    pub ts: Option<String>,
    pub role: Role,
    pub label: String,
    pub title: String,
    pub body: String,
    /// Explains a context edit or missing thinking text.
    pub note: Option<String>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub is_error: Option<bool>,
    pub is_meta: Option<bool>,
    pub is_sidechain: Option<bool>,
    pub prompt_origin: Option<PromptOrigin>,
    pub tokens_est: f64,
    pub tokens_measured: Option<f64>,
    /// Status and, when replaced, the original text.
    pub context_edit: Option<(&'static str, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSubagent {
    pub id: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub turns: usize,
    pub peak_context_tokens: f64,
    pub workflow_phase: Option<String>,
    pub unreadable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportDoc {
    pub session_id: String,
    pub title: String,
    pub provider: Provider,
    pub models: Vec<String>,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub exported_at: String,
    pub redacted: bool,
    pub events: Vec<ExportEvent>,
    pub subagents: Vec<ExportSubagent>,
}

/// What an export stamps in besides the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOptions {
    /// ISO time of the export.
    pub exported_at: String,
    /// The real home dir, collapsed exactly when redacted.
    pub home: Option<String>,
    /// The input was redacted; home paths are then collapsed to `~`.
    pub redacted: bool,
}

fn prepare_event(raw: &TimelineEvent, clean: &dyn Fn(&str) -> String) -> Option<ExportEvent> {
    let mut e = raw.clone();
    e.title = clean(&raw.title);
    e.preview = clean(&raw.preview);
    e.body = clean(&raw.body);
    if let Some(ContextEdit::Replaced {
        original: Some(o), ..
    }) = &mut e.context_edit
    {
        *o = clean(o);
    }
    let body = if e.body.is_empty() {
        e.preview.clone()
    } else {
        e.body.clone()
    };
    if body.is_empty() {
        return None;
    }
    let note = context_edit_note(&e).or_else(|| {
        (e.kind == EventKind::AssistantThinking && e.body.is_empty()).then(|| thinking_note(&e))
    });
    let role = match e.kind {
        EventKind::UserPrompt => Role::User,
        EventKind::AssistantText => Role::Assistant,
        _ => Role::Other,
    };
    let context_edit = e.context_edit.as_ref().map(|c| {
        let original = match c {
            ContextEdit::Replaced { original, .. } => original.clone(),
            ContextEdit::Removed { .. } => None,
        };
        (c.status(), original)
    });
    Some(ExportEvent {
        index: e.index,
        kind: e.kind,
        ts: e.ts.clone(),
        role,
        label: event_badge_label(&e),
        title: e.title.clone(),
        body,
        note,
        tool_name: e.tool_name.clone(),
        tool_use_id: e.tool_use_id.clone(),
        is_error: e.is_error,
        is_meta: e.is_meta,
        is_sidechain: e.is_sidechain,
        prompt_origin: e.prompt_origin,
        tokens_est: e.tokens_est,
        tokens_measured: e.tokens_measured,
        context_edit,
    })
}

/// Reduce a parsed session to the export shape. Drops transcript paths and
/// image refs; skips events with no text.
pub fn prepare_export(session: &ParsedSession, opts: &ExportOptions) -> ExportDoc {
    let collapser = opts
        .redacted
        .then(|| HomeCollapser::new(opts.home.as_deref()));
    let clean = |t: &str| match &collapser {
        Some(c) => c.collapse(t),
        None => t.to_owned(),
    };
    ExportDoc {
        session_id: session.session_id.clone(),
        title: clean(session.title.as_deref().unwrap_or(&session.session_id)),
        provider: session.provider,
        models: session.models.clone(),
        git_branch: session.git_branch.clone(),
        cwd: session.cwd.as_deref().map(clean),
        started_at: session.started_at.clone(),
        ended_at: session.ended_at.clone(),
        exported_at: opts.exported_at.clone(),
        redacted: opts.redacted,
        events: session
            .events
            .iter()
            .filter_map(|e| prepare_event(e, &clean))
            .collect(),
        subagents: session
            .subagents
            .iter()
            .map(|s| ExportSubagent {
                id: s.id.clone(),
                agent_type: s.agent_type.clone(),
                description: s.description.as_deref().map(clean),
                turns: s.turns,
                peak_context_tokens: s.peak_context_tokens,
                workflow_phase: s.workflow_phase.as_deref().map(clean),
                unreadable: s.unreadable,
            })
            .collect(),
    }
}

/// One-line text: whitespace runs become single spaces, trimmed, and long
/// text cut to `max - 1` units plus `…`.
pub fn one_line(text: &str, max: usize) -> String {
    units::clip(
        crate::js::text::trim(&units::collapse_ws(text)),
        max.saturating_sub(1),
    )
}

pub fn one_line_default(text: &str) -> String {
    one_line(text, LINE_MAX)
}

/// Header rows shared by the Markdown and HTML renderers.
pub fn meta_rows(d: &ExportDoc) -> Vec<(&'static str, String)> {
    let exported = format!(
        "{} ({})",
        d.exported_at,
        if d.redacted {
            "redacted"
        } else {
            "not redacted"
        }
    );
    [
        ("Session", Some(d.session_id.clone())),
        ("Provider", Some(d.provider.as_str().to_owned())),
        ("Models", Some(d.models.join(", "))),
        ("Branch", d.git_branch.clone()),
        ("Directory", d.cwd.clone()),
        ("Started", d.started_at.clone()),
        ("Ended", d.ended_at.clone()),
        ("Exported", Some(exported)),
    ]
    .into_iter()
    .filter_map(|(k, v)| {
        v.filter(|v| !v.is_empty())
            .map(|v| (k, one_line(&v, NOTE_MAX)))
    })
    .collect()
}

/// One-line summary of a subagent: type, description, phase, size.
pub fn subagent_summary(s: &ExportSubagent) -> String {
    let size = if s.unreadable {
        "unreadable".to_owned()
    } else {
        format!(
            "{} turns, peak {} tok",
            s.turns,
            crate::js::num::to_js_string(s.peak_context_tokens)
        )
    };
    [
        s.agent_type.clone(),
        s.description.as_deref().map(one_line_default),
        s.workflow_phase
            .as_deref()
            .map(|p| format!("phase {}", one_line_default(p))),
        Some(size),
    ]
    .into_iter()
    .flatten()
    .filter(|p| !p.is_empty())
    .collect::<Vec<_>>()
    .join(", ")
}

/// Who spoke: `User` / `Assistant` for chat, the badge label otherwise.
pub fn who_of(e: &ExportEvent) -> &str {
    match e.role {
        Role::Assistant => "Assistant",
        Role::User if e.label == e.kind.as_str() => "User",
        _ => &e.label,
    }
}

/// Time and flags shown after the speaker, or an empty string.
pub fn tail_of(e: &ExportEvent) -> String {
    [
        e.ts.clone().unwrap_or_default(),
        if e.is_meta == Some(true) { "meta" } else { "" }.to_owned(),
        if e.is_sidechain == Some(true) {
            "sidechain"
        } else {
            ""
        }
        .to_owned(),
        if e.is_error == Some(true) {
            "error"
        } else {
            ""
        }
        .to_owned(),
    ]
    .into_iter()
    .filter(|p| !p.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

/// A backtick fence longer than any backtick run inside `body`.
pub fn fence_for(body: &str) -> String {
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat(3.max(longest + 1))
}
