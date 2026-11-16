//! Claude `attachment` lines: budget category per type, metadata-only types
//! and queued prompts. Ported from the `claude-attachments` TS test; each
//! `attachmentEvent` call there is one parsed attachment line here.
#![allow(clippy::float_cmp)]

mod common;

use common::{analyze_default, jsonl, parse_text};
use peektrace_core::js::units::est_tokens;
use peektrace_core::sessions::model::{
    EventKind, LoadedCategory, ParsedSession, PromptOrigin, TimelineEvent,
};
use serde_json::{Value, json};

fn line(attachment: &Value, text: Option<&str>) -> Value {
    let mut o = json!({"type": "attachment", "attachment": attachment});
    if let Some(t) = text {
        o["rendered"] = json!([{"content": t}]);
    }
    o
}

/// The event one attachment line yields on its own.
fn ev_line(o: &Value) -> TimelineEvent {
    let p = parse_text(&o.to_string());
    let [e] = p.events.as_slice() else {
        panic!("expected one event, got {}", p.events.len())
    };
    e.clone()
}

fn ev(attachment: &Value, text: Option<&str>) -> TimelineEvent {
    ev_line(&line(attachment, text))
}

const TS: &str = "2026-01-01T00:00:00.000Z";

fn parse(rows: &[Value]) -> ParsedSession {
    let rows: Vec<Value> = rows
        .iter()
        .map(|o| {
            let mut o = o.clone();
            o["timestamp"] = json!(TS);
            o["sessionId"] = json!("s");
            o
        })
        .collect();
    parse_text(&jsonl(&rows))
}

#[test]
fn metadata_only_types_cost_nothing_and_load_no_category() {
    let cases = [
        json!({"type":"structured_output","data":{"answer":42},"toolUseID":"toolu_1"}),
        json!({"type":"deferred_tools_record","entries":[{"name":"ToolA"}]}),
        json!({"type":"credential_org","organizationUuid":"org-0000"}),
        json!({"type":"command_permissions","allowedTools":["Bash"]}),
        json!({"type":"max_turns_reached","maxTurns":5,"turnCount":5}),
        json!({"type":"thinking_drop","reason":"org"}),
        json!({"type":"already_read_file","filename":"src/a.ts"}),
        json!({"type":"hook_non_blocking_error","stderr":"boom"}),
        json!({"type":"goals","items":["g"]}),
    ];
    for a in cases {
        let ty = a["type"].as_str().unwrap_or_default();
        let e = ev(&a, None);
        assert_eq!(e.kind, EventKind::Attachment, "{ty}");
        assert_eq!(e.tokens_est, 0.0, "{ty}");
        assert_eq!(e.loaded_category, None, "{ty}");
        assert_eq!(e.attachment_type.as_deref(), Some(ty));
        assert!(!e.body.is_empty(), "{ty}");
    }
}

#[test]
fn a_metadata_type_with_rendered_text_counts_that_text() {
    let text = "<system-reminder>goal: ship it</system-reminder>";
    let e = ev(&json!({"type":"goals","items":["ship it"]}), Some(text));
    assert_eq!(e.tokens_est, est_tokens(text));
    assert!(e.loaded_category.is_some());
}

fn hook(extra: &Value) -> Value {
    let mut h = json!({"type":"hook_success","command":"echo done","stdout":"x".repeat(400),
        "stderr":"","content":""});
    if let (Some(h), Some(extra)) = (h.as_object_mut(), extra.as_object()) {
        h.extend(extra.clone());
    }
    h
}

#[test]
fn a_stop_hook_success_is_not_sent_to_the_model() {
    let e = ev(&hook(&json!({"hookEvent":"Stop","hookName":"Stop"})), None);
    assert_eq!(e.tokens_est, 0.0);
    assert_eq!(e.loaded_category, None);
}

#[test]
fn a_session_start_hook_success_is_context() {
    let e = ev(
        &hook(&json!({"hookEvent":"SessionStart"})),
        Some("<system-reminder>SessionStart hook output</system-reminder>"),
    );
    assert!(e.tokens_est > 0.0);
    assert_eq!(e.loaded_category, Some(LoadedCategory::Reminder));
}

#[test]
fn a_hook_success_without_hook_event_still_counts() {
    let e = ev(&hook(&json!({})), None);
    assert!(e.tokens_est > 0.0);
    assert_eq!(e.loaded_category, Some(LoadedCategory::Reminder));
}

#[test]
fn a_metadata_event_takes_no_share_of_the_measured_delta() {
    let usage = |cache_read: u32| {
        json!({"input_tokens":0,"cache_read_input_tokens":cache_read,
            "cache_creation_input_tokens":0,"output_tokens":10})
    };
    let p = parse(&[
        json!({"type":"user","message":{"role":"user","content":"run it"}}),
        json!({"type":"assistant","requestId":"req_1","message":{"model":"claude-opus-4-5",
            "usage":usage(1000),
            "content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{}}]}}),
        json!({"type":"user","message":{"role":"user",
            "content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}),
        line(
            &json!({"type":"deferred_tools_record","entries":[{"name":"ToolA"}]}),
            None,
        ),
        json!({"type":"assistant","requestId":"req_2","message":{"model":"claude-opus-4-5",
            "usage":usage(1100),"content":[{"type":"text","text":"done"}]}}),
    ]);
    let meta = p
        .events
        .iter()
        .find(|e| e.attachment_type.as_deref() == Some("deferred_tools_record"))
        .expect("meta event");
    let result = common::first_of(&p, EventKind::ToolResult);
    assert_eq!(meta.tokens_measured, None);
    assert_eq!(result.tokens_measured, Some(90.0));
}

#[test]
fn attachment_types_load_their_category() {
    use LoadedCategory::{ClaudeMd, File, Ide, Other, Reminder, Skills};
    let cases = [
        (json!({"type":"total_tokens_reminder"}), Reminder),
        (json!({"type":"silent_turn_reminder"}), Reminder),
        (json!({"type":"output_style","style":"concise"}), Reminder),
        (json!({"type":"output_style_instructions"}), Reminder),
        (json!({"type":"environment","snapshot":{}}), Reminder),
        (json!({"type":"date"}), Reminder),
        (json!({"type":"model","identity":{}}), Reminder),
        (json!({"type":"session_context","context":{}}), Reminder),
        (json!({"type":"auto_mode"}), Reminder),
        (json!({"type":"auto_mode_exit"}), Reminder),
        (json!({"type":"remote_session_change"}), Reminder),
        (json!({"type":"read_truncation_notice"}), Reminder),
        (json!({"type":"plan_mode"}), Reminder),
        (
            json!({"type":"plan_mode_exit","planExists":false}),
            Reminder,
        ),
        (json!({"type":"plan_mode_reentry"}), Reminder),
        (json!({"type":"todo_reminder"}), Reminder),
        (
            json!({"type":"queued_command","commandMode":"task-notification"}),
            Reminder,
        ),
        (
            json!({"type":"compact_file_reference","filename":"src/a.ts"}),
            File,
        ),
        (json!({"type":"skill_listing","content":"s"}), Skills),
        (json!({"type":"nested_memory","content":{}}), ClaudeMd),
        (json!({"type":"file","filename":"a/CLAUDE.md"}), ClaudeMd),
        (json!({"type":"edited_text_file","filename":"a.ts"}), Ide),
        (json!({"type":"task_reminder"}), Reminder),
        (json!({"type":"something_new"}), Other),
    ];
    for (a, category) in cases {
        let ty = a["type"].as_str().unwrap_or_default().to_owned();
        let e = ev(
            &a,
            Some(&format!("<system-reminder>{ty}</system-reminder>")),
        );
        assert_eq!(e.loaded_category, Some(category), "{ty}");
        assert!(e.tokens_est > 0.0, "{ty}");
    }
}

// queued prompts

const QUEUED_TEXT: &str = "also do the second thing";

fn shown() -> String {
    format!(
        "<system-reminder>\nThe user sent a new message while you were working:\n{QUEUED_TEXT}\n</system-reminder>"
    )
}

fn user() -> Value {
    json!({"type":"user","message":{"role":"user","content":"first"}})
}

fn queued(extra: &Value, text: Option<&str>) -> Value {
    let mut a = json!({"type":"queued_command","commandMode":"prompt"});
    if let (Some(a), Some(extra)) = (a.as_object_mut(), extra.as_object()) {
        a.extend(extra.clone());
    }
    let mut o = line(&a, text);
    o["renderedRole"] = json!("system");
    o
}

#[test]
fn a_queued_prompt_is_a_user_message_and_a_notification_is_not() {
    let shown = shown();
    let mut note = line(
        &json!({"type":"queued_command","commandMode":"task-notification",
            "prompt":"<task-notification>done</task-notification>"}),
        Some("<system-reminder>notification</system-reminder>"),
    );
    note["renderedRole"] = json!("system");
    let p = parse(&[
        user(),
        queued(&json!({"prompt": QUEUED_TEXT}), Some(&shown)),
        note,
    ]);
    let prompt = &p.events[1];
    let note = &p.events[2];
    assert_eq!(prompt.kind, EventKind::UserPrompt);
    assert_eq!(prompt.prompt_origin, Some(PromptOrigin::Human));
    assert_eq!(prompt.attachment_type.as_deref(), Some("queued_command"));
    assert_eq!(prompt.body, QUEUED_TEXT);
    assert_eq!(prompt.tokens_est, est_tokens(&shown));
    assert_eq!(note.kind, EventKind::Attachment);
    assert_eq!(analyze_default(p).user_message_count, 2);
}

#[test]
fn a_prompt_rendered_by_its_batch_head_costs_nothing_and_counts() {
    let p = parse(&[
        user(),
        queued(
            &json!({"prompt": QUEUED_TEXT, "renderedByBatchHead": true}),
            None,
        ),
    ]);
    assert_eq!(p.events[1].tokens_est, 0.0);
    assert_eq!(analyze_default(p).user_message_count, 2);
}

#[test]
fn an_is_meta_queued_prompt_is_not_the_users() {
    let p = parse(&[
        user(),
        queued(
            &json!({"prompt": QUEUED_TEXT, "isMeta": true}),
            Some(&shown()),
        ),
    ]);
    assert_eq!(p.events[1].prompt_origin, Some(PromptOrigin::Harness));
    assert_eq!(analyze_default(p).user_message_count, 1);
}

#[test]
fn a_queued_prompt_keeps_its_prompt_source() {
    let e = ev_line(&queued(
        &json!({"prompt": QUEUED_TEXT, "promptSource": "sdk"}),
        Some(&shown()),
    ));
    assert_eq!(e.prompt_origin, Some(PromptOrigin::Sdk));
    assert_eq!(e.prompt_source.as_deref(), Some("sdk"));
}

fn image() -> Value {
    json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD".repeat(500)}})
}

#[test]
fn an_array_prompt_joins_text_blocks_with_image_markers() {
    let e = ev_line(&queued(
        &json!({"prompt":[{"type":"text","text":"part one"}, image(), {"type":"text","text":"part two"}]}),
        None,
    ));
    let body = "part one\n[image: image/png]\npart two";
    assert_eq!(e.kind, EventKind::UserPrompt);
    assert_eq!(e.body, body);
    assert_eq!(e.tokens_est, est_tokens(body));
}

#[test]
fn an_image_only_prompt_is_a_user_message_without_base64() {
    let e = ev_line(&queued(&json!({"prompt":[image()]}), None));
    assert_eq!(e.kind, EventKind::UserPrompt);
    assert_eq!(e.body, "[image: image/png]");
    assert!(!e.body.contains("QUJD"));
    assert_eq!(e.tokens_est, est_tokens("[image: image/png]"));
    assert_eq!(e.loaded_category, None);
}
