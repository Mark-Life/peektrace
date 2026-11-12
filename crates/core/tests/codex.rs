//! Codex parser, header and budget, ported from the TS tests `codex.test.ts`,
//! `codex-budget.test.ts`, the Codex cases of `context-entries.test.ts` and
//! `measure.test.ts`, and the Codex header cases of
//! `swallowed-errors.test.ts`. Same fixtures, same assertions.

mod common;

use common::{analyze_default, file_ref, fixture, read_fixture};
use peektrace_core::codex::{build_header, parse_codex_session};
use peektrace_core::sessions::labels::event_badge_label;
use peektrace_core::sessions::model::{
    BudgetKey, EventKind, LoadedCategory, ParsedSession, TimelineEvent,
};

const SESSION_ID: &str = "019f42f4-342d-7b30-9551-a78e5a2cbe5e";

fn codex(name: &str) -> ParsedSession {
    let rel = format!("sessions-codex/{name}.jsonl");
    parse_codex_session(&read_fixture(&rel), &fixture(&rel), "fallback-id")
}

fn fixture_session() -> ParsedSession {
    codex("rollout-fixture")
}

fn find(p: &ParsedSession, pred: impl Fn(&TimelineEvent) -> bool) -> &TimelineEvent {
    p.events.iter().find(|e| pred(e)).expect("event")
}

fn at(p: &ParsedSession, index: usize) -> Vec<&TimelineEvent> {
    p.events.iter().filter(|e| e.index == index).collect()
}

// codex.test.ts

#[test]
fn extracts_provider_and_session_metadata_from_session_meta() {
    let p = fixture_session();
    assert_eq!(p.provider.as_str(), "codex");
    assert_eq!(p.session_id, SESSION_ID);
    assert_eq!(
        p.cwd.as_deref(),
        Some("/Users/andrey-m/Code/personal/peephole")
    );
    assert_eq!(p.git_branch.as_deref(), Some("feature/parsers"));
    assert_eq!(p.version.as_deref(), Some("0.134.0"));
    assert_eq!(p.models, ["gpt-5.5"]);
    assert_eq!(p.title, None);
    assert_eq!(p.compaction_indexes.len(), 0);
    assert_eq!(p.subagents.len(), 0);
    assert_eq!(p.started_at.as_deref(), Some("2026-07-08T10:00:00.000Z"));
    assert_eq!(p.ended_at.as_deref(), Some("2026-07-08T10:00:12.000Z"));
}

#[test]
fn turn0_uses_last_token_usage_not_the_cumulative_total() {
    let p = fixture_session();
    let t = &p.turns[0];
    assert_eq!(t.context_tokens, 13_917.0);
    assert_eq!(t.cache_read_tokens, 11_648.0);
    assert_eq!(t.input_tokens, 13_917.0 - 11_648.0);
    assert_eq!(t.cache_creation_tokens, 0.0);
    assert_eq!(t.output_tokens, 201.0);
    assert_eq!(t.model, "gpt-5.5");
    assert_eq!(t.ts.as_deref(), Some("2026-07-08T10:00:06.000Z"));
    assert_eq!(
        t.input_tokens + t.cache_read_tokens + t.cache_creation_tokens,
        t.context_tokens
    );
    assert_eq!(t.event_indexes, [2, 3, 4]);
}

#[test]
fn tool_call_and_result_pair_by_call_id() {
    let p = fixture_session();
    let call = find(&p, |e| {
        e.kind == EventKind::ToolCall && e.tool_name.as_deref() == Some("exec_command")
    });
    let result = find(&p, |e| {
        e.kind == EventKind::ToolResult && e.tool_use_id.as_deref() == Some("call_1")
    });
    assert_eq!(call.tool_use_id.as_deref(), Some("call_1"));
    assert_eq!(result.tool_use_id, call.tool_use_id);
    assert_eq!(result.is_error, Some(true));
}

#[test]
fn custom_tool_call_apply_patch_pairs_with_its_output_no_false_error() {
    let p = fixture_session();
    let call = find(&p, |e| {
        e.kind == EventKind::ToolCall && e.tool_name.as_deref() == Some("apply_patch")
    });
    let result = find(&p, |e| {
        e.kind == EventKind::ToolResult && e.tool_use_id.as_deref() == Some("call_2")
    });
    assert_eq!(call.tool_use_id.as_deref(), Some("call_2"));
    assert!(result.body.contains("Success"));
    assert_eq!(result.is_error, Some(false));
}

#[test]
fn header_derives_project_from_cwd_basename_and_stamps_agent_codex() {
    let rel = "sessions-codex/rollout-fixture.jsonl";
    let text = read_fixture(rel);
    let size = u64::try_from(text.len()).expect("size");
    let h = build_header(&text, &file_ref(SESSION_ID, "", &fixture(rel), size));
    assert_eq!(h.agent.as_str(), "codex");
    assert_eq!(h.project.as_str(), "peephole");
    assert_eq!(
        h.cwd.as_deref(),
        Some("/Users/andrey-m/Code/personal/peephole")
    );
    assert_eq!(h.git_branch.as_deref(), Some("feature/parsers"));
    assert_eq!(h.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(h.message_count, 3);
    assert_eq!(h.started_at.as_deref(), Some("2026-07-08T10:00:00.000Z"));
    assert!(h.size_bytes > 0);
}

#[test]
fn never_throws_on_a_garbage_or_empty_transcript() {
    let broken = [
        "not json at all",
        r#"{"type":"response_item","payload":{"#,
        "",
        "   ",
        r#"{"type":"event_msg","payload":{"type":"token_count""#,
    ]
    .join("\n");
    let p = parse_codex_session(&broken, "/tmp/broken.jsonl", "broken");
    assert_eq!(p.provider.as_str(), "codex");
    assert_eq!(p.session_id, "broken");
    assert_eq!(p.events.len(), 0);
    assert_eq!(p.turns.len(), 0);
    assert_eq!(p.native_context_window, None);

    let empty = parse_codex_session("", "/tmp/empty.jsonl", "empty");
    assert_eq!(empty.events.len(), 0);
    assert_eq!(empty.turns.len(), 0);
}

// codex-budget.test.ts

fn budget() -> ParsedSession {
    codex("rollout-budget")
}

fn fork() -> ParsedSession {
    codex("rollout-fork")
}

#[test]
fn context_is_input_tokens_cached_input_is_a_subset() {
    let p = budget();
    let t = &p.turns[0];
    assert_eq!(t.context_tokens, 12_000.0);
    assert_eq!(t.cache_read_tokens, 3000.0);
    assert_eq!(t.input_tokens, 9000.0);
    assert_eq!(t.output_tokens, 220.0);
}

#[test]
fn a_calls_output_events_carry_its_request_id_inputs_do_not() {
    let p = budget();
    let tagged: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.request_id.as_deref() == Some("turn-a#1"))
        .map(|e| e.kind.as_str())
        .collect();
    assert_eq!(
        tagged,
        ["assistant-thinking", "assistant-text", "tool-call"]
    );
    assert!(
        p.events
            .iter()
            .filter(|e| matches!(
                e.kind,
                EventKind::UserPrompt | EventKind::ToolResult | EventKind::Attachment
            ))
            .all(|e| e.request_id.is_none())
    );
    for t in &p.turns {
        for &i in &t.event_indexes {
            let ev = &p.events[usize::try_from(i).expect("index")];
            assert_eq!(ev.request_id.as_deref(), Some(t.request_id.as_str()));
        }
    }
}

#[test]
fn output_of_an_aborted_call_is_not_paired_with_a_later_call() {
    let p = budget();
    let aborted = find(&p, |e| {
        e.kind == EventKind::AssistantThinking && e.body == "**Planning alias**"
    });
    assert_eq!(aborted.request_id, None);
}

#[test]
fn every_call_gets_a_snapshot_whose_slices_sum_to_its_context() {
    let a = analyze_default(budget());
    let ctx: Vec<f64> = a.snapshots.iter().map(|s| s.ctx).collect();
    assert_eq!(ctx, [12_000.0, 13_100.0, 13_500.0, 13_900.0, 14_000.0]);
    for s in &a.snapshots {
        let sum: f64 = BudgetKey::ALL.into_iter().map(|k| s.slices.get(k)).sum();
        assert!((sum - s.ctx).abs() < 5e-7, "{sum} vs {}", s.ctx);
    }
}

#[test]
fn peak_budget_uses_the_model_context_window_and_is_not_empty() {
    let a = analyze_default(budget());
    assert_eq!(a.context_window, 258_400.0);
    assert!(!a.context_window_inferred);
    assert_eq!(a.peak_context_tokens, 14_000.0);
    assert_ne!(a.budget.len(), 0);
    let tokens = |key| {
        a.budget
            .iter()
            .find(|b| b.key == key)
            .map_or(0.0, |b| b.tokens)
    };
    assert!(tokens(BudgetKey::ToolResults) > 0.0);
    assert!(tokens(BudgetKey::Thinking) > 0.0);
}

#[test]
fn system_floor_is_the_first_calls_context_minus_earlier_inputs() {
    let p = budget();
    let first_call = p
        .events
        .iter()
        .position(|e| e.request_id.as_deref() == Some("turn-a#1"))
        .expect("first call");
    let before: f64 = p.events[..first_call]
        .iter()
        .filter(|e| e.kind != EventKind::SystemPrompt)
        .map(|e| e.tokens_est)
        .sum();
    assert_eq!(analyze_default(p).system_overhead_tokens, 12_000.0 - before);
}

#[test]
fn a_turn_with_no_events_of_its_own_still_yields_a_peak_budget() {
    let mut p = budget();
    for e in &mut p.events {
        if e.request_id.as_deref() == Some("turn-c#2") {
            e.request_id = None;
        }
    }
    let a = analyze_default(p);
    assert_eq!(a.snapshots.len(), 4);
    assert_ne!(a.budget.len(), 0);
}

#[test]
fn fork_the_parents_replayed_token_count_is_not_a_turn() {
    let p = fork();
    let ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(ids, ["child-turn#1", "child-turn#2"]);
    assert_eq!(p.turns[0].context_tokens, 10_000.0);
    let output: f64 = p.turns.iter().map(|t| t.output_tokens).sum();
    assert_eq!(output, 160.0);
}

#[test]
fn fork_the_first_turn_is_the_childs_first_call_and_has_a_snapshot() {
    let p = fork();
    let kinds: Vec<_> = p.turns[0]
        .event_indexes
        .iter()
        .map(|&i| p.events[usize::try_from(i).expect("index")].kind.as_str())
        .collect();
    assert_eq!(kinds, ["assistant-thinking", "tool-call"]);
    let a = analyze_default(p);
    let turns: Vec<_> = a.snapshots.iter().map(|s| s.turn_index).collect();
    assert_eq!(turns, [0, 1]);
    assert!(a.system_overhead_tokens > 0.0);
}

// measure.test.ts, Codex rollout

#[test]
fn tool_output_gets_input_growth_minus_the_calls_output() {
    let p = budget();
    assert_eq!(at(&p, 11)[0].tokens_measured, Some(880.0));
    assert_eq!(at(&p, 15)[0].tokens_measured, Some(260.0));
}

#[test]
fn a_step_holding_an_aborted_calls_output_keeps_estimates() {
    let p = budget();
    assert_eq!(at(&p, 24)[0].tokens_measured, None);
    assert_eq!(at(&p, 29)[0].tokens_measured, None);
}

// context-entries.test.ts, Codex

fn context() -> ParsedSession {
    let rel = "sessions-codex/rollout-context.jsonl";
    parse_codex_session(&read_fixture(rel), &fixture(rel), "fallback")
}

#[test]
fn base_instructions_and_dynamic_tools_become_the_system_prompt() {
    let p = context();
    let sys = &p.events[0];
    assert_eq!(sys.kind, EventKind::SystemPrompt);
    assert_eq!(sys.attachment_type.as_deref(), Some("base_instructions"));
    assert!(sys.body.contains("You are Codex, a coding agent."));
    assert!(sys.body.contains("## dynamic tools (1)"));
    assert!(sys.body.contains("open_panel"));
    assert_eq!(event_badge_label(sys), "system prompt");
}

#[test]
fn developer_parts_are_split_and_labelled_by_tag() {
    let p = context();
    let dev = at(&p, 2);
    let types: Vec<_> = dev
        .iter()
        .map(|e| e.attachment_type.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(types, ["permissions_instructions", "skills_instructions"]);
    let cats: Vec<_> = dev.iter().map(|e| e.loaded_category).collect();
    assert_eq!(
        cats,
        [Some(LoadedCategory::Other), Some(LoadedCategory::Skills)]
    );
}

#[test]
fn agents_md_and_environment_are_context_not_a_user_prompt() {
    let p = context();
    let injected = at(&p, 3);
    let kinds: Vec<_> = injected.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EventKind::Attachment, EventKind::Attachment]);
    let types: Vec<_> = injected
        .iter()
        .map(|e| e.attachment_type.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(types, ["agents_md", "environment_context"]);
    assert_eq!(injected[0].loaded_category, Some(LoadedCategory::ClaudeMd));
    let prompts: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.kind == EventKind::UserPrompt)
        .map(|e| e.body.as_str())
        .collect();
    assert_eq!(prompts, ["Add a test."]);
}

#[test]
fn inter_agent_messages_are_kept() {
    let p = context();
    let msg = find(&p, |e| {
        e.attachment_type.as_deref() == Some("agent_message")
    });
    assert_eq!(msg.title, "message from /root");
    assert_eq!(msg.body, "Check the parser.");
}

// swallowed-errors.test.ts, Codex header messageCount

fn item(role: &str, texts: &[&str]) -> String {
    let content: Vec<_> = texts
        .iter()
        .map(|t| serde_json::json!({"type": "input_text", "text": t}))
        .collect();
    serde_json::json!({
        "type": "response_item",
        "payload": {"type": "message", "role": role, "content": content},
    })
    .to_string()
}

fn message_count(text: &str) -> u64 {
    build_header(text, &file_ref("good", "", "/x.jsonl", 1)).message_count
}

#[test]
fn user_items_that_only_carry_injected_parts_do_not_count() {
    let text = [
        item(
            "developer",
            &["<permissions instructions>x</permissions instructions>"],
        ),
        item("user", &["# AGENTS.md instructions for /demo\n\nbe brief"]),
        item(
            "user",
            &["<environment_context>\n  <cwd>/demo</cwd>\n</environment_context>"],
        ),
        item("user", &["hello"]),
        item("assistant", &["hi"]),
    ]
    .join("\n");
    assert_eq!(message_count(&text), 2);
}

#[test]
fn a_user_item_mixing_an_injected_part_with_typed_text_counts() {
    let text = item(
        "user",
        &["<environment_context>x</environment_context>", "hello"],
    );
    assert_eq!(message_count(&text), 1);
}
