//! Ports of `apps/inspector/test/transcript-row.test.ts`.

use std::hash::DefaultHasher;

use peektrace_protocol::view::{ContextEdit, EventKind, PromptOrigin};

use super::*;
use crate::model::fixtures::kind;

fn event() -> TimelineEvent {
    let mut built = kind(EventKind::ToolCall);
    built.index = 7;
    built.ts = Some("2026-08-08T15:00:00.000Z".into());
    built.request_id = Some("req_1".into());
    built.is_sidechain = Some(false);
    built.title = "Bash".into();
    built.preview = "ls -la /workspace".into();
    built.body = r#"{"command":"ls -la /workspace"}"#.into();
    built.tokens_est = 42.0;
    built.tool_name = Some("Bash".into());
    built.tool_use_id = Some("toolu_1".into());
    built
}

type Change = fn(&mut TimelineEvent);

/// One change per field a table row renders.
const ROW_CHANGES: [(&str, Change); 12] = [
    ("index", |e| e.index += 1),
    ("kind", |e| e.kind = EventKind::ToolResult),
    ("toolName", |e| e.tool_name = Some("Bash-changed".into())),
    ("attachmentType", |e| {
        e.attachment_type = Some("undefined-changed".into());
    }),
    ("isSidechain", |e| e.is_sidechain = Some(true)),
    ("promptOrigin", |e| {
        e.prompt_origin = Some(PromptOrigin::Harness);
    }),
    ("promptSource", |e| {
        e.prompt_source = Some("undefined-changed".into());
    }),
    ("preview", |e| e.preview.push_str("-changed")),
    ("tokensEst", |e| e.tokens_est += 1.0),
    ("tokensMeasured", |e| e.tokens_measured = Some(1.0)),
    ("thinkingDurationMs", |e| e.thinking_duration_ms = Some(1.0)),
    ("body", |e| e.body.push_str("-changed")),
];

/// What a chat leaf renders on top of the row fields.
const CHAT_CHANGES: [(&str, Change); 3] = [
    ("title", |e| e.title.push_str("-changed")),
    ("loadedCategory", |e| {
        e.loaded_category = Some("undefined-changed".into());
    }),
    ("isError", |e| e.is_error = Some(true)),
];

fn changed(f: Change) -> TimelineEvent {
    let mut e = event();
    f(&mut e);
    e
}

fn hash(e: &TimelineEvent) -> u64 {
    let mut h = DefaultHasher::new();
    hash_chat_event(e, &mut h);
    h.finish()
}

#[test]
fn a_reanalysed_fresh_copy_is_still_the_same_row() {
    assert!(same_event(&event(), &event().clone()));
    assert_eq!(hash(&event()), hash(&event().clone()));
}

#[test]
fn every_field_the_row_renders_forces_a_rerender() {
    for (field, f) in ROW_CHANGES {
        assert!(!same_event(&event(), &changed(f)), "{field}");
    }
}

#[test]
fn fields_the_row_never_renders_do_not_force_a_rerender() {
    let e = event();
    let ts = {
        let mut built = event();
        built.ts = Some("2026-01-01T00:00:00.000Z".into());
        built
    };
    let request = {
        let mut built = event();
        built.request_id = Some("req_2".into());
        built
    };
    let title = {
        let mut built = event();
        built.title = "Other".into();
        built
    };
    assert!(same_event(&e, &ts));
    assert!(same_event(&e, &request));
    assert!(same_event(&e, &title));
}

#[test]
fn a_context_edit_is_compared_by_value() {
    let edited = {
        let mut built = event();
        built.context_edit = Some(ContextEdit {
            status: "replaced".into(),
        });
        built
    };
    assert!(same_event(&edited, &edited.clone()));
    assert!(!same_event(&event(), &edited));
    let removed = {
        let mut built = edited.clone();
        built.context_edit = Some(ContextEdit {
            status: "removed".into(),
        });
        built
    };
    assert!(!same_chat_event(&edited, &removed));
    assert_ne!(hash(&edited), hash(&removed));
}

#[test]
fn an_attachment_that_changes_type_rerenders_its_badge() {
    let listing = {
        let mut built = event();
        built.kind = EventKind::Attachment;
        built.tool_name = None;
        built.attachment_type = Some("skill_listing".into());
        built
    };
    let todo = {
        let mut built = listing.clone();
        built.attachment_type = Some("todo".into());
        built
    };
    assert!(!same_event(&listing, &todo));
    assert!(same_event(&listing, &listing.clone()));
}

#[test]
fn an_appended_event_is_never_mistaken_for_an_existing_one() {
    let appended = {
        let mut built = event();
        built.index = 8;
        built.preview = "next".into();
        built
    };
    assert!(!same_event(&event(), &appended));
}

#[test]
fn every_field_a_chat_leaf_renders_forces_a_rerender() {
    for (field, f) in ROW_CHANGES.into_iter().chain(CHAT_CHANGES) {
        let c = changed(f);
        assert!(!same_chat_event(&event(), &c), "{field}");
        assert_ne!(hash(&event()), hash(&c), "{field}");
    }
}

#[test]
fn a_chip_label_is_a_chat_only_field() {
    let relabelled = {
        let mut built = event();
        built.title = "Other".into();
        built
    };
    assert!(same_event(&event(), &relabelled));
    assert!(!same_chat_event(&event(), &relabelled));
}

#[test]
fn fields_no_leaf_renders_still_do_not_force_a_rerender() {
    let ts = {
        let mut built = event();
        built.ts = Some("2026-01-01T00:00:00.000Z".into());
        built
    };
    let request = {
        let mut built = event();
        built.request_id = Some("req_2".into());
        built
    };
    assert!(same_chat_event(&event(), &ts));
    assert!(same_chat_event(&event(), &request));
    assert_eq!(hash(&event()), hash(&ts));
    assert_eq!(hash(&event()), hash(&request));
}
