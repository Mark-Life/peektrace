//! Token sizes of events across later context edits (see [`ContextEdit`]).
//! The budget walk counts the original size until the first edit's line,
//! then each edit's size from its own line on.

use std::collections::HashMap;

use super::model::{ContextEdit, EventKind, TimelineEvent};

fn measured_or_est(e: &TimelineEvent) -> f64 {
    e.tokens_measured.unwrap_or(e.tokens_est)
}

/// Size the model saw before the edit.
pub fn tokens_before(e: &TimelineEvent) -> f64 {
    match &e.context_edit {
        Some(ContextEdit::Replaced {
            original_tokens_est,
            ..
        }) => *original_tokens_est,
        _ => measured_or_est(e),
    }
}

/// Size the model sees after the edit (0 once removed).
pub fn tokens_after(e: &TimelineEvent) -> f64 {
    match &e.context_edit {
        Some(ContextEdit::Removed { .. }) => 0.0,
        _ => measured_or_est(e),
    }
}

/// One point where an event's context size changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeStep {
    pub at_index: f64,
    pub tokens_est: f64,
}

/// Each edit of the event in line order with the size it leaves; the last
/// is the final edit. Empty when the event was never edited.
pub fn size_steps(e: &TimelineEvent) -> Vec<SizeStep> {
    let Some(edit) = &e.context_edit else {
        return Vec::new();
    };
    edit.steps()
        .iter()
        .map(|s| SizeStep {
            at_index: s.at_index,
            tokens_est: s.tokens_est,
        })
        .chain(std::iter::once(SizeStep {
            at_index: edit.at_index(),
            tokens_est: tokens_after(e),
        }))
        .collect()
}

/// Size the model saw when sent the request at line `line`.
pub fn tokens_at(e: &TimelineEvent, line: f64) -> f64 {
    size_steps(e)
        .iter()
        .rev()
        .find(|s| s.at_index < line)
        .map_or_else(|| tokens_before(e), |s| s.tokens_est)
}

fn is_visible(e: &TimelineEvent) -> bool {
    matches!(e.kind, EventKind::AssistantText | EventKind::ToolCall)
}

/// A model call's retained size from line `at_index` on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RequestStep {
    pub at_index: f64,
    /// Thinking left in context (replacement thinking blocks).
    pub thinking: f64,
    pub visible: f64,
}

/// Visible (text and tool call) size of one model call, and its edits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestSize {
    /// Edits of this call's content in line order; the last is the final one.
    pub steps: Vec<RequestStep>,
    pub visible: f64,
}

/// Size a call's event leaves after its final edit: (visible, thinking).
fn final_size(e: &TimelineEvent) -> (f64, f64) {
    if matches!(e.context_edit, Some(ContextEdit::Replaced { .. })) {
        let visible = if is_visible(e) { e.tokens_est } else { 0.0 };
        let thinking = if e.kind == EventKind::AssistantThinking {
            e.tokens_est
        } else {
            0.0
        };
        return (visible, thinking);
    }
    (if is_visible(e) { tokens_after(e) } else { 0.0 }, 0.0)
}

fn add_event(size: &mut RequestSize, e: &TimelineEvent) {
    match &e.context_edit {
        Some(ContextEdit::Replaced {
            original_tokens_est,
            ..
        }) => size.visible += original_tokens_est,
        _ if is_visible(e) => size.visible += e.tokens_est,
        _ => {}
    }
    let Some(edit) = &e.context_edit else {
        return;
    };
    let (visible, thinking) = final_size(e);
    let points: Vec<RequestStep> = edit
        .steps()
        .iter()
        .map(|s| RequestStep {
            at_index: s.at_index,
            visible: s.tokens_est,
            thinking: s.thinking_est,
        })
        .chain(std::iter::once(RequestStep {
            at_index: edit.at_index(),
            visible,
            thinking,
        }))
        .collect();
    if size.steps.is_empty() {
        size.steps = points
            .iter()
            .map(|p| RequestStep {
                visible: 0.0,
                thinking: 0.0,
                ..*p
            })
            .collect();
    }
    for (step, p) in size.steps.iter_mut().zip(&points) {
        step.visible += p.visible;
        step.thinking += p.thinking;
    }
}

/// Per request id, the visible size of its assistant output and its edits.
pub fn request_sizes(events: &[TimelineEvent]) -> HashMap<&str, RequestSize> {
    let mut sizes: HashMap<&str, RequestSize> = HashMap::new();
    for e in events {
        if let Some(r) = e.request_id.as_deref().filter(|r| !r.is_empty()) {
            add_event(sizes.entry(r).or_default(), e);
        }
    }
    sizes
}
