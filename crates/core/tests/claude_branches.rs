//! Claude transcripts are trees (`parentUuid`): rewinds leave dead branches.
//! Ported from the `claude-branches` TS test.
#![allow(clippy::float_cmp)]

mod common;

use common::{of_kind, parse_rows};
use peektrace_core::sessions::model::{EventKind, ParsedSession};
use serde_json::{Value, json};

fn usage(ctx: u32) -> Value {
    json!({"input_tokens":1,"cache_read_input_tokens":ctx,"cache_creation_input_tokens":0,"output_tokens":5})
}

fn parent(p: Option<&str>) -> Value {
    p.map_or(Value::Null, |p| json!(p))
}

fn user(uuid: &str, parent_uuid: Option<&str>, content: &str) -> Value {
    json!({"type":"user","uuid":uuid,"parentUuid":parent(parent_uuid),
        "timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":content}})
}

fn assistant_with(uuid: &str, parent_uuid: &str, req: &str, ctx: u32, content: &Value) -> Value {
    json!({"type":"assistant","uuid":uuid,"parentUuid":parent_uuid,"requestId":req,
        "timestamp":"2026-01-01T00:00:01Z",
        "message":{"model":"claude-opus-9","usage":usage(ctx),"content":content}})
}

fn assistant(uuid: &str, parent_uuid: &str, req: &str, ctx: u32) -> Value {
    assistant_with(
        uuid,
        parent_uuid,
        req,
        ctx,
        &json!([{"type":"text","text":"ok"}]),
    )
}

fn tool_use(id: &str) -> Value {
    json!([{"type":"tool_use","id":id,"name":"Read","input":{}}])
}

fn tool_result(uuid: &str, parent_uuid: &str, tool_use_id: &str) -> Value {
    json!({"type":"user","uuid":uuid,"parentUuid":parent_uuid,"sourceToolAssistantUUID":parent_uuid,
        "timestamp":"2026-01-01T00:00:02Z","message":{"role":"user",
            "content":[{"type":"tool_result","tool_use_id":tool_use_id,"content":"r"}]}})
}

/// Request ids of turns on the timeline: dead-branch turns have no events.
fn live(p: &ParsedSession) -> Vec<&str> {
    p.turns
        .iter()
        .filter(|t| !t.event_indexes.is_empty())
        .map(|t| t.request_id.as_str())
        .collect()
}

fn rewound() -> ParsedSession {
    parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        user("u2", Some("a1"), "second"),
        assistant("a2", "u2", "r2", 900),
        user("u2b", Some("a1"), "second, edited"),
        assistant("a3", "u2b", "r3", 200),
    ])
}

#[test]
fn drops_the_abandoned_branch_after_a_rewind() {
    assert_eq!(live(&rewound()), ["r1", "r3"]);
}

#[test]
fn keeps_billed_usage_of_the_dead_branch_without_events() {
    let p = rewound();
    let ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(ids, ["r1", "r2", "r3"]);
    let dead = p.turns.iter().find(|t| t.request_id == "r2").expect("r2");
    assert_eq!(dead.cache_read_tokens, 900.0);
    assert_eq!(dead.event_indexes, Vec::<i64>::new());
    assert!(
        p.events
            .iter()
            .all(|e| e.request_id.as_deref() != Some("r2"))
    );
}

#[test]
fn drops_tool_calls_and_results_on_the_dead_branch() {
    let p = parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        user("u2", Some("a1"), "second"),
        assistant_with("a2", "u2", "r2", 900, &tool_use("T1")),
        tool_result("t1", "a2", "T1"),
        assistant("a2b", "t1", "r2b", 950),
        user("u2b", Some("a1"), "second, edited"),
        assistant("a3", "u2b", "r3", 200),
    ]);
    assert_eq!(live(&p), ["r1", "r3"]);
    assert_eq!(of_kind(&p, EventKind::ToolResult).len(), 0);
    assert_eq!(of_kind(&p, EventKind::ToolCall).len(), 0);
}

#[test]
fn a_duplicated_prompt_row_is_not_a_fork() {
    let p = parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        user("u2", Some("a1"), "second"),
        user("u2", Some("a1"), "second"),
        assistant("a2", "u2", "r2", 150),
    ]);
    assert_eq!(live(&p), ["r1", "r2"]);
}

#[test]
fn ignores_sidechain_prompts_sharing_a_parent() {
    let side = |uuid: &str| {
        json!({"type":"user","uuid":uuid,"parentUuid":"a1","isSidechain":true,
            "timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"side"}})
    };
    let p = parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        user("u2", Some("a1"), "second"),
        side("s1"),
        side("s2"),
        assistant("a2", "u2", "r2", 150),
    ]);
    assert_eq!(live(&p), ["r1", "r2"]);
}

#[test]
fn dead_rows_still_feed_session_meta() {
    let p = parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        json!({"type":"user","uuid":"u2","parentUuid":"a1","cwd":"/synthetic/repo",
            "timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"second"}}),
        assistant("a2", "u2", "r2", 900),
        user("u2b", Some("a1"), "second, edited"),
        json!({"type":"assistant","uuid":"a3","parentUuid":"u2b","requestId":"r3",
            "timestamp":"2026-01-01T00:00:09Z","message":{"model":"claude-opus-9","usage":usage(200),
                "content":[{"type":"text","text":"ok"}]}}),
    ]);
    assert_eq!(live(&p), ["r1", "r3"]);
    assert_eq!(p.cwd.as_deref(), Some("/synthetic/repo"));
    assert_eq!(p.ended_at.as_deref(), Some("2026-01-01T00:00:09Z"));
}

#[test]
fn falls_back_to_every_row_when_the_last_row_would_be_dead() {
    let p = parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        user("u2", Some("a1"), "second"),
        user("u2b", Some("a1"), "second, edited"),
        assistant("a2", "u2", "r2", 150),
    ]);
    assert_eq!(live(&p), ["r1", "r2"]);
}

#[test]
fn keeps_every_result_of_parallel_tool_calls() {
    let p = parse_rows(&[
        user("u1", None, "go"),
        assistant_with("a1", "u1", "r1", 100, &tool_use("T1")),
        assistant_with("a1b", "a1", "r1", 100, &tool_use("T2")),
        tool_result("t1", "a1", "T1"),
        tool_result("t2", "a1b", "T2"),
        assistant("a2", "t2", "r2", 150),
    ]);
    assert_eq!(of_kind(&p, EventKind::ToolResult).len(), 2);
    assert_eq!(live(&p), ["r1", "r2"]);
}

#[test]
fn keeps_turns_before_a_compaction() {
    let p = parse_rows(&[
        user("u1", None, "first"),
        assistant("a1", "u1", "r1", 100),
        json!({"type":"system","subtype":"compact_boundary","uuid":"c1","parentUuid":null,
            "logicalParentUuid":"a1","timestamp":"2026-01-01T00:00:03Z",
            "compactMetadata":{"trigger":"auto","preTokens":100}}),
        user("u2", Some("c1"), "after"),
        assistant("a2", "u2", "r2", 50),
    ]);
    assert_eq!(live(&p), ["r1", "r2"]);
}
