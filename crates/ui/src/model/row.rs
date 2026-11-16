//! When two renders of a transcript row are the same row. Re-analysing a
//! live session rebuilds every event, so rows compare the fields they
//! render instead of the analysis revision.

use std::hash::{Hash, Hasher};
use std::mem::discriminant;

use peektrace_protocol::view::TimelineEvent;

/// True when both events would render an identical table row: `index`,
/// `kind`, `toolName`, `attachmentType`, `isSidechain`, `promptOrigin`,
/// `promptSource`, `preview`, `tokensEst`, `tokensMeasured`,
/// `thinkingDurationMs`, `body`, and the context edit by value (the view
/// types keep only its `status`).
pub fn same_event(a: &TimelineEvent, b: &TimelineEvent) -> bool {
    a.index == b.index
        && a.kind == b.kind
        && a.tool_name == b.tool_name
        && a.attachment_type == b.attachment_type
        && a.is_sidechain == b.is_sidechain
        && a.prompt_origin == b.prompt_origin
        && a.prompt_source == b.prompt_source
        && a.preview == b.preview
        && a.tokens_est.to_bits() == b.tokens_est.to_bits()
        && a.tokens_measured.map(f64::to_bits) == b.tokens_measured.map(f64::to_bits)
        && a.thinking_duration_ms.map(f64::to_bits) == b.thinking_duration_ms.map(f64::to_bits)
        && a.body == b.body
        && a.context_edit == b.context_edit
}

/// [`same_event`] plus what a chat leaf adds: the chip label (`title`), the
/// chip icon (`loadedCategory`) and the error tint (`isError`).
pub fn same_chat_event(a: &TimelineEvent, b: &TimelineEvent) -> bool {
    same_event(a, b)
        && a.title == b.title
        && a.loaded_category == b.loaded_category
        && a.is_error == b.is_error
}

/// Hashes exactly the fields [`same_chat_event`] compares.
pub fn hash_chat_event(e: &TimelineEvent, h: &mut impl Hasher) {
    (
        e.index,
        e.kind,
        &e.tool_name,
        &e.attachment_type,
        e.is_sidechain,
    )
        .hash(h);
    e.prompt_origin.map(|o| discriminant(&o)).hash(h);
    (&e.prompt_source, &e.preview, &e.body).hash(h);
    (
        e.tokens_est.to_bits(),
        e.tokens_measured.map(f64::to_bits),
        e.thinking_duration_ms.map(f64::to_bits),
    )
        .hash(h);
    e.context_edit.as_ref().map(|c| &c.status).hash(h);
    (&e.title, &e.loaded_category, e.is_error).hash(h);
}

#[cfg(test)]
mod tests;
