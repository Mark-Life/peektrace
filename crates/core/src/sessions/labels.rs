//! Human-facing labels for timeline events, shared by every renderer.

use super::model::{ContextEdit, EventKind, PromptOrigin, TimelineEvent};
use crate::js::{num, text};

/// The badge label for one row: the tool name, else `system prompt`, else
/// who wrote a prompt the user did not type, else the attachment type as
/// words, else the event kind.
pub fn event_badge_label(e: &TimelineEvent) -> String {
    if let Some(name) = &e.tool_name {
        return name.clone();
    }
    if e.kind == EventKind::SystemPrompt {
        return "system prompt".to_owned();
    }
    if e.kind == EventKind::UserPrompt
        && let Some(origin) = e.prompt_origin
        && !matches!(origin, PromptOrigin::Human | PromptOrigin::Sdk)
    {
        return origin.as_str().replace('-', " ");
    }
    let ty = e.attachment_type.as_deref().map_or("", text::trim);
    if ty.is_empty() {
        e.kind.as_str().to_owned()
    } else {
        ty.replace('_', " ")
    }
}

/// One paragraph on a context edit, with the original text when kept.
pub fn context_edit_note(e: &TimelineEvent) -> Option<String> {
    let edit = e.context_edit.as_ref()?;
    let lines: Vec<String> = edit
        .steps()
        .iter()
        .map(|s| s.at_index)
        .chain(std::iter::once(edit.at_index()))
        .map(|at| num::to_js_string(at + 1.0))
        .collect();
    let first = lines.first().cloned().unwrap_or_default();
    let earlier = if lines.len() > 1 {
        let many = lines.len() > 2;
        format!(
            " Earlier edit{} at line{} {}.",
            if many { "s" } else { "" },
            if many { "s" } else { "" },
            lines[..lines.len() - 1].join(", ")
        )
    } else {
        String::new()
    };
    let at = num::to_js_string(edit.at_index() + 1.0);
    Some(match edit {
        ContextEdit::Removed { .. } => format!(
            "Dropped from model context by the edit at line {at}.{earlier} Later turns do not see it."
        ),
        ContextEdit::Replaced {
            original,
            original_tokens_est,
            ..
        } => {
            let head = format!(
                "Edited in model context at line {at}.{earlier} Turns before line {first} saw the original"
            );
            match original {
                None => format!("{head}."),
                Some(o) => format!(
                    "{head} (~{} tok):\n\n{o}",
                    num::to_js_string(*original_tokens_est)
                ),
            }
        }
    })
}

/// What to say about a thinking block whose text the transcript did not keep.
pub fn thinking_note(e: &TimelineEvent) -> String {
    match e.tokens_measured {
        None => "Thinking content is not stored in the transcript (only a signature). Its token cost is in the timeline 'thinking' band.".to_owned(),
        Some(n) => format!(
            "Thinking text is not stored in the transcript (only a signature). Usage reports {} thinking tokens for it; they stay in the context window on later turns.",
            num::to_js_string(n)
        ),
    }
}
