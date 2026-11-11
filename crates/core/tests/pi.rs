//! Pi parser and header, ported from the TS tests `pi.test.ts`,
//! `pi-branches.test.ts`, the Pi cases of `context-entries.test.ts` and the
//! Pi cases of `cost.test.ts`. Same fixtures, same assertions.

mod common;

use std::fs;

use common::{analyzed, file_ref, fixture, read, temp_dir};
use peektrace_core::claude::{self, Outcome, ParseArgs, parse_claude_session};
use peektrace_core::pi::{build_header, parse_pi_session};
use peektrace_core::sessions::model::{EventKind, ParsedSession, TimelineEvent};
use serde_json::{Value, json};

const PI_ID: &str = "019f3788-3bb1-7302-a2bf-55b30354d752";

fn pi_fixture() -> ParsedSession {
    let rel = "sessions-pi/pi-fixture.jsonl";
    parse_pi_session(&read(rel), &fixture(rel), "fallback-id")
}

fn find(p: &ParsedSession, pred: impl Fn(&TimelineEvent) -> bool) -> &TimelineEvent {
    p.events.iter().find(|e| pred(e)).expect("event")
}

fn of_kind(p: &ParsedSession, kind: EventKind) -> Vec<&TimelineEvent> {
    p.events.iter().filter(|e| e.kind == kind).collect()
}

// pi.test.ts

#[test]
fn normalizes_provider_and_session_metadata() {
    let p = pi_fixture();
    assert_eq!(p.provider.as_str(), "pi");
    assert_eq!(p.session_id, PI_ID);
    assert_eq!(p.cwd.as_deref(), Some("/Users/demo/proj"));
    assert_eq!(p.version.as_deref(), Some("3"));
    assert_eq!(p.started_at.as_deref(), Some("2026-07-06T13:05:02.897Z"));
    assert_eq!(p.ended_at.as_deref(), Some("2026-07-06T13:05:40.000Z"));
    assert_eq!(p.compaction_indexes.len(), 0);
    assert_eq!(p.subagents.len(), 0);
    assert_eq!(p.git_branch, None);
    assert_eq!(p.title, None);
}

#[test]
fn one_turn_per_assistant_line_with_non_zero_usage() {
    let p = pi_fixture();
    let mut ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["aaaa1111", "bbbb2222"]);
}

#[test]
fn token_math_context_is_input_plus_cache_read_plus_cache_write() {
    let p = pi_fixture();
    let t = p
        .turns
        .iter()
        .find(|t| t.request_id == "aaaa1111")
        .expect("turn");
    assert_eq!(t.input_tokens, 11.0);
    assert_eq!(t.cache_read_tokens, 5750.0);
    assert_eq!(t.cache_creation_tokens, 0.0);
    assert_eq!(t.output_tokens, 35.0);
    assert_eq!(t.context_tokens, 11.0 + 5750.0);
    assert_eq!(t.context_tokens, 5796.0 - 35.0);
    assert_eq!(t.model, "claude-opus-4-8");
    assert_eq!(t.ts.as_deref(), Some("2026-07-06T13:05:20.000Z"));
}

#[test]
fn cache_write_feeds_cache_creation_tokens_on_the_second_turn() {
    let p = pi_fixture();
    let t = p
        .turns
        .iter()
        .find(|t| t.request_id == "bbbb2222")
        .expect("turn");
    assert_eq!(t.cache_creation_tokens, 100.0);
    assert_eq!(t.context_tokens, 20.0 + 100.0);
}

#[test]
fn tool_call_and_tool_result_pair_by_tool_use_id_and_tool_name() {
    let p = pi_fixture();
    let call = find(&p, |e| e.kind == EventKind::ToolCall);
    let result = find(&p, |e| e.kind == EventKind::ToolResult);
    assert_eq!(call.tool_use_id.as_deref(), Some("call_1"));
    assert_eq!(call.tool_name.as_deref(), Some("read"));
    assert_eq!(result.tool_use_id.as_deref(), Some("call_1"));
    assert_eq!(result.tool_name.as_deref(), Some("read"));
    assert!(call.body.contains("README.md"));
}

#[test]
fn tool_result_body_joins_content_text_blocks() {
    let p = pi_fixture();
    let result = find(&p, |e| e.kind == EventKind::ToolResult);
    assert!(result.body.contains("A demo project readme body."));
    assert_eq!(result.is_error, Some(false));
}

#[test]
fn empty_assistant_thinking_uses_the_placeholder_preview_and_0_tokens() {
    let p = pi_fixture();
    let thinking = of_kind(&p, EventKind::AssistantThinking);
    assert_eq!(thinking.len(), 2);
    let empty = thinking
        .iter()
        .find(|e| e.body.is_empty())
        .expect("empty thinking");
    assert_eq!(empty.tokens_est, 0.0);
    assert_eq!(empty.preview, "(content not stored in transcript)");
}

#[test]
fn user_prompts_and_assistant_text_are_captured() {
    let p = pi_fixture();
    let prompts: Vec<_> = of_kind(&p, EventKind::UserPrompt)
        .into_iter()
        .map(|e| e.body.as_str())
        .collect();
    assert_eq!(
        prompts,
        [
            "read the readme and summarize it",
            "thanks, now do it again"
        ]
    );
    assert_ne!(of_kind(&p, EventKind::AssistantText).len(), 0);
}

#[test]
fn never_throws_on_garbage_yields_empty_events_and_turns() {
    let garbage = [
        "not json at all",
        r#"{"type":"message","message":{"#,
        "",
        "   ",
        r#"{"type":"session""#,
    ]
    .join("\n");
    let p = parse_pi_session(&garbage, "/tmp/garbage.jsonl", "g1");
    assert_eq!(p.provider.as_str(), "pi");
    assert_eq!(p.session_id, "g1");
    assert_eq!(p.events.len(), 0);
    assert_eq!(p.turns.len(), 0);
}

#[test]
fn empty_transcript_yields_a_valid_empty_parsed_session() {
    let p = parse_pi_session("", "/tmp/empty.jsonl", "e1");
    assert_eq!(p.events.len(), 0);
    assert_eq!(p.turns.len(), 0);
    assert_eq!(p.models.len(), 0);
}

#[test]
fn header_stamps_the_pi_agent_cwd_first_model_and_counts() {
    let rel = "sessions-pi/pi-fixture.jsonl";
    let text = read(rel);
    let size = u64::try_from(text.len()).expect("size");
    let h = build_header(
        &text,
        &file_ref(PI_ID, "--Users-demo-proj--", &fixture(rel), size),
    );
    assert_eq!(h.agent.as_str(), "pi");
    assert_eq!(h.project.as_str(), "--Users-demo-proj--");
    assert_eq!(h.cwd.as_deref(), Some("/Users/demo/proj"));
    assert_eq!(h.model.as_deref(), Some("qwen/qwen3.6-27b"));
    assert_eq!(h.message_count, 5);
    assert_eq!(h.started_at.as_deref(), Some("2026-07-06T13:05:02.897Z"));
    assert!(h.size_bytes > 0);
}

#[test]
fn header_tolerates_a_fully_garbage_transcript() {
    let h = build_header(
        "garbage\n{broken\n\n!!!",
        &file_ref("h", "", "/tmp/x.jsonl", 20),
    );
    assert_eq!(h.id.as_str(), "h");
    assert_eq!(h.model, None);
    assert_eq!(h.message_count, 0);
    assert_eq!(h.malformed_lines, Some(2));
}

// pi-branches.test.ts

const TS: &str = "2026-01-01T00:00:00.000Z";

/// The `parentId` of a row: `null`, no key at all, or an id.
#[derive(Clone, Copy)]
enum Parent<'a> {
    Root,
    Absent,
    Id(&'a str),
}

fn with_parent(mut row: Value, parent: Parent<'_>) -> String {
    match parent {
        Parent::Root => row["parentId"] = Value::Null,
        Parent::Id(id) => row["parentId"] = json!(id),
        Parent::Absent => {}
    }
    row.to_string()
}

fn user(id: &str, parent: Parent<'_>, text: &str) -> String {
    with_parent(
        json!({"type": "message", "id": id, "timestamp": TS,
            "message": {"role": "user", "content": [{"type": "text", "text": text}]}}),
        parent,
    )
}

fn assistant(id: &str, parent: Parent<'_>, cache_read: u32) -> String {
    with_parent(
        json!({"type": "message", "id": id, "timestamp": TS,
            "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}],
                "model": "demo-model",
                "usage": {"input": 1, "output": 2, "cacheRead": cache_read, "cacheWrite": 0}}}),
        parent,
    )
}

fn header_line() -> String {
    json!({"type": "session", "version": 3, "id": "pi-synthetic", "timestamp": TS,
        "cwd": "/synthetic/proj"})
    .to_string()
}

fn parse_lines(lines: &[String]) -> ParsedSession {
    parse_pi_session(&lines.join("\n"), "/synthetic/pi.jsonl", "fallback")
}

/// Request ids of turns on the timeline: abandoned-branch turns have no events.
fn live(p: &ParsedSession) -> Vec<&str> {
    p.turns
        .iter()
        .filter(|t| !t.event_indexes.is_empty())
        .map(|t| t.request_id.as_str())
        .collect()
}

use Parent::{Absent, Id, Root};

#[test]
fn follows_the_active_branch_from_the_leaf() {
    let p = parse_lines(&[
        header_line(),
        user("u1", Root, "first"),
        assistant("a1", Id("u1"), 100),
        user("u2", Id("a1"), "second"),
        assistant("a2", Id("u2"), 900),
        json!({"type": "branch_summary", "id": "bs", "parentId": "a1", "fromId": "a2",
            "timestamp": TS, "summary": "tried something"})
        .to_string(),
        user("u3", Id("bs"), "second, again"),
        assistant("a3", Id("u3"), 200),
    ]);
    assert_eq!(live(&p), ["a1", "a3"]);
}

#[test]
fn keeps_billed_usage_off_the_active_branch_in_turns_with_no_events() {
    let p = parse_lines(&[
        header_line(),
        user("u1", Root, "first"),
        assistant("a1", Id("u1"), 100),
        user("u2", Id("a1"), "second"),
        assistant("a2", Id("u2"), 900),
        user("u3", Id("a1"), "second, again"),
        assistant("a3", Id("u3"), 200),
    ]);
    assert_eq!(live(&p), ["a1", "a3"]);
    let ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(ids, ["a1", "a2", "a3"]);
    let dead = p
        .turns
        .iter()
        .find(|t| t.request_id == "a2")
        .expect("dead turn");
    assert_eq!(dead.cache_read_tokens, 900.0);
    assert_eq!(dead.event_indexes.len(), 0);
    assert!(
        !p.events
            .iter()
            .any(|e| e.request_id.as_deref() == Some("a2"))
    );
}

#[test]
fn falls_back_to_file_order_when_entries_have_no_parent_id() {
    let p = parse_lines(&[
        header_line(),
        user("u1", Absent, "first"),
        assistant("a1", Absent, 100),
        user("u2", Absent, "second"),
        assistant("a2", Absent, 200),
    ]);
    assert_eq!(live(&p), ["a1", "a2"]);
}

#[test]
fn ignores_a_context_edit_on_the_dead_branch() {
    let p = parse_lines(&[
        header_line(),
        user("u1", Root, "first"),
        assistant("a1", Id("u1"), 100),
        user("u2", Id("a1"), "second"),
        json!({"type": "context_edit", "id": "e1", "parentId": "u2", "targetId": "u1",
            "replacement": null, "timestamp": TS})
        .to_string(),
        user("u3", Id("a1"), "second, again"),
        assistant("a3", Id("u3"), 200),
    ]);
    let first = find(&p, |e| e.index == 1);
    assert_eq!(first.context_edit, None);
    assert_eq!(live(&p), ["a1", "a3"]);
}

#[test]
fn falls_back_to_file_order_when_a_parent_is_missing() {
    let p = parse_lines(&[
        header_line(),
        user("u1", Root, "first"),
        assistant("a1", Id("u1"), 100),
        user("u2", Id("a1"), "second"),
        assistant("a2", Id("u2"), 900),
        user("u3", Id("gone"), "orphan"),
        assistant("a3", Id("u3"), 200),
    ]);
    assert_eq!(live(&p), ["a1", "a2", "a3"]);
}

#[test]
fn a_session_info_title_off_the_branch_still_applies() {
    let p = parse_lines(&[
        header_line(),
        user("u1", Root, "first"),
        assistant("a1", Id("u1"), 100),
        json!({"type": "session_info", "id": "i1", "parentId": "a1",
            "name": "Synthetic title", "timestamp": TS})
        .to_string(),
        user("u3", Id("a1"), "second"),
        assistant("a3", Id("u3"), 200),
    ]);
    assert_eq!(p.title.as_deref(), Some("Synthetic title"));
    assert_eq!(live(&p), ["a1", "a3"]);
}

// context-entries.test.ts, Pi

fn context() -> ParsedSession {
    let rel = "sessions-pi/pi-context.jsonl";
    parse_pi_session(&read(rel), &fixture(rel), "fallback")
}

#[test]
fn the_system_message_renders_every_named_section_and_its_tools() {
    let p = context();
    let sys = find(&p, |e| e.kind == EventKind::SystemPrompt);
    assert_eq!(sys.title, "System prompt");
    assert_eq!(
        sys.preview,
        "preamble \u{b7} project_context \u{b7} cwd \u{b7} tools (1)"
    );
    assert!(
        sys.body
            .contains("## preamble\n\nYou are pi, a coding agent.")
    );
    assert!(sys.body.contains("### read"));
}

#[test]
fn a_later_system_message_is_an_update_and_names_removed_sections() {
    let p = context();
    let updates = of_kind(&p, EventKind::SystemPrompt);
    let titles: Vec<_> = updates.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "System prompt",
            "System prompt update",
            "System prompt update"
        ]
    );
    assert!(updates[2].body.contains("(section \"cwd\" removed)"));
    assert!(updates[2].body.contains("Be brief."));
}

#[test]
fn a_compactions_new_system_message_sits_below_the_compaction() {
    let p = context();
    let pos = p
        .events
        .iter()
        .position(|e| e.kind == EventKind::Compaction)
        .expect("compaction");
    let next = &p.events[pos + 1];
    assert_eq!(next.kind, EventKind::SystemPrompt);
    assert_eq!(next.index, p.events[pos].index);
    assert!(next.body.contains("Resume from the summary."));
}

#[test]
fn session_info_names_the_session() {
    assert_eq!(context().title.as_deref(), Some("read the readme"));
}

#[test]
fn string_user_content_and_images_are_kept() {
    let p = context();
    assert_eq!(
        find(&p, |e| e.body == "read the readme").kind,
        EventKind::UserPrompt
    );
    assert_eq!(
        find(&p, |e| e.attachment_type.as_deref() == Some("image")).body,
        "[image: image/png]"
    );
    assert_eq!(
        find(&p, |e| e.kind == EventKind::ToolResult).body,
        "screenshot:[image: image/jpeg]"
    );
}

#[test]
fn custom_messages_and_in_context_bash_runs_are_attachments() {
    let p = context();
    let custom = find(&p, |e| {
        e.attachment_type.as_deref() == Some("custom_message")
    });
    assert_eq!(custom.title, "todo-reminder");
    let bash: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.attachment_type.as_deref() == Some("bash_execution"))
        .collect();
    let titles: Vec<_> = bash.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["$ ls"]);
    assert!(bash[0].body.contains("README.md"));
}

#[test]
fn compaction_and_branch_summaries_are_wrapped_as_the_model_sees_them() {
    let p = context();
    let compaction = find(&p, |e| e.kind == EventKind::Compaction);
    assert!(compaction.body.contains("User asked to read the readme."));
    assert!(compaction.body.ends_with("</summary>"));
    assert_eq!(p.compaction_indexes, [compaction.index]);
    assert_eq!(
        find(&p, |e| e.kind == EventKind::Summary).title,
        "Branch summary"
    );
}

#[test]
fn a_system_prompt_stays_inside_the_measured_system_floor() {
    let p = context();
    let mut without = p.clone();
    without.events.retain(|e| e.kind != EventKind::SystemPrompt);
    let a = analyzed(p);
    let first = &a.snapshots[0];
    assert_eq!(first.slices.system_tools, a.system_overhead_tokens);
    assert_eq!(first.slices.other, 0.0);
    assert_eq!(
        a.system_overhead_tokens,
        analyzed(without).system_overhead_tokens
    );
}

// cost.test.ts, Pi

fn close(a: Option<f64>, b: f64, digits: i32) -> bool {
    a.is_some_and(|a| (a - b).abs() < 10f64.powi(-digits) / 2.0)
}

#[test]
fn pi_a_repeated_message_id_counts_once() {
    let message = json!({"type": "message", "id": "m1", "message": {
        "role": "assistant", "model": "claude-sonnet-4-6",
        "usage": {"input": 1_000_000, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "content": []}})
    .to_string();
    let text = [message.as_str(), message.as_str()].join("\n");
    let h = build_header(&text, &file_ref("s", "demo", "/synthetic/s.jsonl", 1));
    assert!(close(h.cost_usd, 3.0, 6), "{:?}", h.cost_usd);
}

fn claude_header(text: &str) -> Option<f64> {
    let dir = temp_dir("claude-cost");
    let path = dir.join("s.jsonl");
    fs::write(&path, text).expect("write");
    let path = path.to_string_lossy().into_owned();
    let out = claude::load_headers(&[file_ref("s", "demo", &path, 1)], &tracing::Span::none());
    let _ = fs::remove_dir_all(&dir);
    match out.into_iter().next() {
        Some(Outcome::Header(h)) => h.cost_usd,
        _ => panic!("no header"),
    }
}

#[test]
fn claude_and_pi_headers_match_analyze_on_the_same_transcript() {
    let split = json!({"input_tokens": 2000, "output_tokens": 300,
        "cache_read_input_tokens": 50_000, "cache_creation_input_tokens": 4000,
        "cache_creation": {"ephemeral_1h_input_tokens": 3000, "ephemeral_5m_input_tokens": 1000}});
    let mut split_900 = split.clone();
    split_900["output_tokens"] = json!(900);
    let usage = |input: u32, output: u32| {
        json!({"input_tokens": input, "output_tokens": output,
            "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0})
    };
    let claude = [
        json!({"type": "user", "timestamp": "2026-01-01T00:00:00Z", "message": {"content": "hi"}}),
        json!({"type": "assistant", "timestamp": "2026-01-01T00:00:01Z", "requestId": "r1",
            "message": {"model": "claude-opus-4-8", "usage": split,
                "content": [{"type": "text", "text": "ok"}]}}),
        json!({"type": "assistant", "requestId": "r1",
            "message": {"model": "claude-opus-4-8", "usage": split_900}}),
        json!({"type": "assistant", "requestId": "r2", "isSidechain": true,
            "message": {"model": "claude-haiku-4-5", "usage": usage(7000, 50)}}),
        json!({"type": "assistant", "requestId": "r3",
            "message": {"model": "<synthetic>", "usage": usage(0, 0)}}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let a = analyzed(parse_claude_session(&ParseArgs {
        text: &claude,
        path: "/synthetic/s.jsonl",
        session_id: "s",
        include_sidechain_turns: false,
        skip_system_prompt: false,
    }));
    let total = a.cost.and_then(|c| c.total_usd).expect("claude cost");
    assert!(close(claude_header(&claude), total, 10), "{total}");

    let pi_message = |id: &str, input: u32| {
        json!({"type": "message", "id": id, "timestamp": "2026-01-01T00:00:01Z", "message": {
            "role": "assistant", "model": "claude-sonnet-4-6",
            "usage": {"input": input, "output": 20, "cacheRead": 900, "cacheWrite": 100},
            "content": [{"type": "text", "text": "ok"}]}})
        .to_string()
    };
    let pi = [
        pi_message("m1", 500),
        pi_message("m1", 9999),
        pi_message("m2", 0),
    ]
    .join("\n");
    let a = analyzed(parse_pi_session(&pi, "/synthetic/p.jsonl", "p"));
    let total = a.cost.and_then(|c| c.total_usd).expect("pi cost");
    let h = build_header(&pi, &file_ref("s", "demo", "/synthetic/s.jsonl", 1));
    assert!(close(h.cost_usd, total, 10), "{:?} vs {total}", h.cost_usd);
}
