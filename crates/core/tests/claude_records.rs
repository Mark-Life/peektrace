//! Claude record types, usage fields and server tools. Ported from
//! `parse-records`, `record-types`, `thinking-tokens`, `streamed-output`,
//! `server-tools`, `first-line` and `defensive` TS tests.
#![allow(clippy::float_cmp)]

mod common;

use common::{
    analyze_default, first_of, jsonl, of_kind, parse_fixture, parse_rows, parse_sidechain,
    parse_text, read_fixture,
};

use peektrace_core::sessions::model::EventKind;
use serde_json::{Value, json};

// parse-records

#[test]
fn summary_line_becomes_a_rolling_summary_event() {
    let p =
        parse_rows(&[json!({"type":"summary","summary":"Fixed the build\nmore","timestamp":"t1"})]);
    let e = first_of(&p, EventKind::Summary);
    assert_eq!(e.title, "Rolling summary");
    assert_eq!(e.preview, "Fixed the build");
    assert_eq!(e.body, "Fixed the build\nmore");
    assert!(e.tokens_est > 0.0);
}

#[test]
fn generic_system_row_keeps_subtype_in_the_title() {
    let p = parse_rows(&[json!({"type":"system","subtype":"demo_notice","content":"heads up"})]);
    let e = first_of(&p, EventKind::System);
    assert_eq!(e.title, "system: demo_notice");
    assert_eq!(e.body, "heads up");
    assert_eq!(e.tokens_est, 0.0);
}

#[test]
fn advisor_result_plain_and_redacted() {
    let p = parse_rows(&[json!({
        "type": "assistant", "requestId": "r1",
        "message": {"id": "m1", "content": [
            {"type":"advisor_tool_result","tool_use_id":"u1","content":{"type":"advisor_result","text":"try X"}},
            {"type":"advisor_tool_result","tool_use_id":"u2","content":{"type":"advisor_redacted_result"}}
        ]}
    })]);
    let got: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.title == "advisor result")
        .map(|e| (e.tool_use_id.clone(), e.body.clone(), e.tokens_est > 0.0))
        .collect();
    assert_eq!(
        got,
        [
            (Some("u1".to_owned()), "try X".to_owned(), true),
            (
                Some("u2".to_owned()),
                "(advisor response encrypted, not stored in transcript)".to_owned(),
                false
            ),
        ]
    );
}

fn fetch_result_body(content: &Value) -> Option<String> {
    let p = parse_rows(&[json!({
        "type": "assistant", "requestId": "r2",
        "message": {"id": "m2", "content": [
            {"type":"web_fetch_tool_result","tool_use_id":"f1","content": content}
        ]}
    })]);
    p.events
        .iter()
        .find(|e| e.tool_use_id.as_deref() == Some("f1") && e.title.ends_with("result"))
        .map(|e| e.body.clone())
}

#[test]
fn web_fetch_text_source_is_appended_under_the_url_and_title() {
    let body = fetch_result_body(&json!({
        "type":"web_fetch_result","url":"https://example.test/doc",
        "content":{"title":"Doc","source":{"type":"text","data":"body"}}
    }));
    assert_eq!(
        body.as_deref(),
        Some("https://example.test/doc — Doc\n\nbody")
    );
}

#[test]
fn web_fetch_without_text_source_keeps_only_the_head() {
    let body = fetch_result_body(&json!({
        "type":"web_fetch_result","url":"https://example.test/doc",
        "content":{"title":"Doc","source":{"type":"base64","data":"AAAA"}}
    }));
    assert_eq!(body.as_deref(), Some("https://example.test/doc — Doc"));
}

// record-types

fn assistant(request_id: &str, model: &str, ts: &str, usage: &Value, text: &str) -> Value {
    json!({
        "type": "assistant", "timestamp": ts, "requestId": request_id,
        "message": {"model": model, "usage": usage, "content": [{"type":"text","text": text}]}
    })
}

fn lines() -> Vec<Value> {
    let mut synthetic = assistant(
        "r0",
        "<synthetic>",
        "2026-01-01T00:00:01Z",
        &json!({"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":0}),
        "API Error: synthetic",
    );
    synthetic["isApiErrorMessage"] = json!(true);
    vec![
        json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"content":"hi"}}),
        synthetic,
        assistant(
            "r1",
            "claude-opus-9",
            "2026-01-01T00:00:02Z",
            &json!({"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000,
                "cache_creation":{"ephemeral_1h_input_tokens":2000,"ephemeral_5m_input_tokens":1000},
                "output_tokens":50}),
            "ok",
        ),
        json!({"type":"system","subtype":"compact_boundary","uuid":"b1","timestamp":"2026-01-01T00:00:03Z",
            "content":"Conversation compacted",
            "compactMetadata":{"trigger":"manual","preTokens":3010,"postTokens":400,"durationMs":1000}}),
        json!({"type":"user","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-01-01T00:00:04Z",
            "message":{"content":"Summary of the earlier conversation."}}),
        json!({"type":"api-request","timestamp":"2026-01-01T00:00:05Z"}),
        json!({"type":"last-prompt"}),
        json!({"type":"queue-operation","operation":"enqueue"}),
        json!({"type":"file-history-snapshot"}),
        json!({"type":"permission-mode"}),
        json!({"type":"cost-state","modelUsage":{"claude-opus-9[1m]":{"inputTokens":10,"outputTokens":50}}}),
        json!({"type":"brand-new-record"}),
        json!({"type":"brand-new-record"}),
        json!({"type":"system","subtype":"brand_new_subtype","content":"x"}),
    ]
}

fn joined(rows: &[Value]) -> String {
    format!("{}\n", jsonl(rows))
}

fn parsed_all() -> peektrace_core::sessions::model::ParsedSession {
    parse_text(&joined(&lines()))
}

#[test]
fn cache_write_split_lands_on_the_turn() {
    let p = parsed_all();
    let t = p.turns.iter().find(|t| t.request_id == "r1").expect("r1");
    assert_eq!(t.cache_creation_tokens, 3000.0);
    assert_eq!(t.cache_creation_1h_tokens, Some(2000.0));
    assert_eq!(t.cache_creation_5m_tokens, Some(1000.0));
}

#[test]
fn compact_boundary_tokens_land_on_the_compaction_event() {
    let p = parsed_all();
    let c = first_of(&p, EventKind::Compaction);
    assert_eq!(c.compact_pre_tokens, Some(3010.0));
    assert_eq!(c.compact_post_tokens, Some(400.0));
}

#[test]
fn synthetic_is_not_listed_as_a_model() {
    let p = parsed_all();
    assert_eq!(p.models, ["claude-opus-9"]);
    assert!(p.turns.iter().all(|t| t.model != "<synthetic>"));
}

#[test]
fn only_new_record_types_and_subtypes_count_as_unknown() {
    let p = parsed_all();
    assert_eq!(
        p.unknown_record_types,
        Some(vec![("brand-new-record".to_owned(), 2.0)])
    );
    assert_eq!(
        p.unknown_system_subtypes,
        Some(vec![("brand_new_subtype".to_owned(), 1.0)])
    );
}

#[test]
fn one_million_model_usage_key_sets_the_native_window() {
    let p = parsed_all();
    assert_eq!(p.native_context_window, Some(1_000_000.0));
    assert!(!analyze_default(p).context_window_inferred);
}

#[test]
fn a_session_with_only_synthetic_has_no_model() {
    let l = lines();
    let p = parse_text(&joined(&l[..2]));
    assert_eq!(p.models, Vec::<String>::new());
}

#[test]
fn a_compaction_with_no_boundary_carries_no_tokens() {
    let p = parse_text(&joined(&[lines()[4].clone()]));
    let c = first_of(&p, EventKind::Compaction);
    assert_eq!(c.compact_pre_tokens, None);
    assert_eq!(c.compact_post_tokens, None);
}

#[test]
fn a_boundary_is_used_by_one_summary_only() {
    let l = lines();
    let p = parse_text(&joined(&[l[3].clone(), l[4].clone(), l[4].clone()]));
    let pre: Vec<_> = of_kind(&p, EventKind::Compaction)
        .iter()
        .map(|e| e.compact_pre_tokens)
        .collect();
    assert_eq!(pre, [Some(3010.0), None]);
}

#[test]
fn cost_state_without_one_million_key_leaves_the_window_inferred() {
    let p = parse_text(&joined(&[
        lines()[2].clone(),
        json!({"type":"cost-state","modelUsage":{"claude-opus-9":{"inputTokens":10}}}),
    ]));
    assert_eq!(p.native_context_window, None);
    assert!(analyze_default(p).context_window_inferred);
}

#[test]
fn a_streamed_duplicate_keeps_the_first_cache_split() {
    let dup = assistant(
        "r1",
        "claude-opus-9",
        "2026-01-01T00:00:02Z",
        &json!({"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":3000},
            "output_tokens":80}),
        "ok",
    );
    let p = parse_text(&joined(&[lines()[2].clone(), dup]));
    let t = &p.turns[0];
    assert_eq!(t.cache_creation_1h_tokens, Some(2000.0));
    assert_eq!(t.cache_creation_5m_tokens, Some(1000.0));
    assert_eq!(t.output_tokens, 80.0);
}

#[test]
fn a_reported_zero_split_is_kept_and_a_missing_one_stays_absent() {
    let usage = json!({"input_tokens":10,"cache_creation_input_tokens":5,"output_tokens":1});
    let mut with_split = usage.clone();
    with_split["cache_creation"] =
        json!({"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":5});
    let p = parse_text(&joined(&[
        assistant("a", "m", "t", &usage, "ok"),
        assistant("b", "m", "t", &with_split, "ok"),
    ]));
    assert_eq!(p.turns[0].cache_creation_1h_tokens, None);
    assert_eq!(p.turns[1].cache_creation_1h_tokens, Some(0.0));
}

#[test]
fn untyped_lines_and_known_subtypes_are_not_counted() {
    let p = parse_text(&joined(&[
        json!({"foo":1}),
        json!({"type":7}),
        json!({"type":"system","subtype":"turn_duration"}),
        json!({"type":"custom-title"}),
    ]));
    assert_eq!(p.unknown_record_types, None);
    assert_eq!(p.unknown_system_subtypes, None);
}

#[test]
fn record_types_claude_code_writes_are_not_counted() {
    let mut rows: Vec<Value> = [
        "agent-color",
        "agent-setting",
        "tag",
        "worktree-state",
        "content-replacement",
        "memory-mode",
        "progress",
        "pr-link",
    ]
    .iter()
    .map(|t| json!({"type": t}))
    .collect();
    rows.push(json!({"type":"system","subtype":"api_error"}));
    let p = parse_text(&joined(&rows));
    assert_eq!(p.unknown_record_types, None);
    assert_eq!(p.unknown_system_subtypes, None);
}

// thinking-tokens

#[test]
fn reported_thinking_tokens_land_on_the_thinking_block() {
    let p = parse_fixture("sessions-claude/thinking-tokens.jsonl");
    let t = first_of(&p, EventKind::AssistantThinking);
    assert_eq!(t.body, "");
    assert_eq!(t.tokens_measured, Some(400.0));
    assert_eq!(t.thinking_duration_ms, Some(4200.0));
    let thinking: Vec<_> = p.turns.iter().map(|t| t.thinking_tokens).collect();
    assert_eq!(thinking, [Some(400.0), None]);
    let effort: Vec<_> = p.turns.iter().map(|t| t.effort.as_deref()).collect();
    assert_eq!(effort, [Some("high"), None]);
}

#[test]
fn thinking_slice_is_the_reported_count() {
    let a = analyze_default(parse_fixture("sessions-claude/thinking-tokens.jsonl"));
    let last = a.snapshots.last().expect("snapshot");
    assert_eq!(last.slices.thinking, 400.0);
    assert_eq!(last.slices.assistant_text, 100.0);
}

// streamed-output

fn streamed() -> peektrace_core::sessions::model::ParsedSession {
    parse_sidechain(&read_fixture("sessions-claude/streamed-output.jsonl"))
}

#[test]
fn a_turn_keeps_the_last_lines_output_and_thinking() {
    let p = streamed();
    let out: Vec<_> = p.turns.iter().map(|t| t.output_tokens).collect();
    assert_eq!(out, [300.0, 15.0]);
    let th: Vec<_> = p.turns.iter().map(|t| t.thinking_tokens).collect();
    assert_eq!(th, [Some(200.0), None]);
}

#[test]
fn a_server_tool_count_on_a_later_line_is_kept() {
    let p = streamed();
    let s: Vec<_> = p.turns.iter().map(|t| t.web_search_requests).collect();
    assert_eq!(s, [Some(1.0), None]);
    let f: Vec<_> = p.turns.iter().map(|t| t.web_fetch_requests).collect();
    assert_eq!(f, [None, None]);
}

#[test]
fn input_and_cache_usage_are_unchanged_across_lines() {
    let p = streamed();
    let ctx: Vec<_> = p.turns.iter().map(|t| t.context_tokens).collect();
    assert_eq!(ctx, [4010.0, 4340.0]);
    let cr: Vec<_> = p.turns.iter().map(|t| t.cache_read_tokens).collect();
    assert_eq!(cr, [0.0, 4010.0]);
}

#[test]
fn the_thinking_block_is_sized_from_the_final_count() {
    let p = streamed();
    assert_eq!(
        first_of(&p, EventKind::AssistantThinking).tokens_measured,
        Some(200.0)
    );
}

#[test]
fn the_budget_splits_final_output_into_thinking_and_text() {
    let a = analyze_default(streamed());
    assert_eq!(a.snapshots[1].slices.thinking, 200.0);
}

// server-tools

#[test]
fn search_hits_and_fetch_errors_link_to_their_call() {
    let p = parse_fixture("sessions-claude/server-tools.jsonl");
    let results = of_kind(&p, EventKind::ToolResult);
    let got: Vec<_> = results
        .iter()
        .map(|e| {
            (
                e.title.as_str(),
                e.tool_use_id.as_deref(),
                e.is_error.unwrap_or(false),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("web_search result", Some("srv_1"), false),
            ("web_fetch result (error)", Some("srv_2"), true),
        ]
    );
    assert_eq!(
        results[0].body,
        "- [Bun 1.4](https://bun.sh/blog/bun-v1.4) · 3 days ago"
    );
    assert_eq!(results[1].body, "error: url_not_accessible");
}

#[test]
fn other_server_tool_results_keep_their_text() {
    let p = parse_rows(&[json!({
        "type":"assistant","requestId":"r9",
        "message":{"model":"m","content":[
            {"type":"code_execution_tool_result","tool_use_id":"srv_9","content":[{"type":"text","text":"42"}]}
        ],"usage":{"input_tokens":1,"output_tokens":1}}
    })]);
    let r = &p.events[0];
    assert_eq!(r.title, "code_execution result");
    assert_eq!(r.body, "42");
    assert_eq!(r.server_tool, Some(true));
}

#[test]
fn server_tool_usage_lands_on_the_turn_and_sums() {
    let p = parse_fixture("sessions-claude/server-tools.jsonl");
    assert_eq!(p.turns[0].web_search_requests, Some(1.0));
    assert_eq!(p.turns[0].web_fetch_requests, Some(1.0));
    let a = analyze_default(p);
    assert_eq!(a.web_search_requests, 1.0);
    assert_eq!(a.web_fetch_requests, 1.0);
}
