//! Context entries, prompt origins, measured sizes and compaction buckets.
//! Ported from the Claude cases of `context-entries`, `prompt-origin`,
//! `measure` and `compaction-buckets` TS tests.
#![allow(clippy::float_cmp)]

mod common;

use common::{analyze_default, of_kind, parse_fixture, parse_rows};
use peektrace_core::sessions::labels::event_badge_label;
use peektrace_core::sessions::measure::measure_tokens;
use peektrace_core::sessions::model::{
    ContextEdit, EventKind, EventShape, LoadedCategory, ParsedSession, PromptOrigin, Provider,
    TimelineEvent, Turn, TurnShape, TurnsCost,
};
use serde_json::{Value, json};

// context-entries (Claude)

const SNAPSHOT: &str = "sessions-claude/prompt-snapshot.jsonl";

fn without_prompt(p: &ParsedSession) -> ParsedSession {
    ParsedSession {
        events: p
            .events
            .iter()
            .filter(|e| e.kind != EventKind::SystemPrompt)
            .cloned()
            .collect(),
        ..p.clone()
    }
}

#[test]
fn prompt_snapshot_becomes_a_system_prompt_without_the_marker() {
    let p = parse_fixture(SNAPSHOT);
    let first = of_kind(&p, EventKind::SystemPrompt)[0];
    assert_eq!(first.title, "System prompt");
    assert_eq!(
        first.body,
        "You are an interactive agent.\n\n# Environment\n - cwd: /repo"
    );
    assert!(!first.body.contains("__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__"));
}

#[test]
fn repeat_snapshots_add_only_what_changed() {
    let p = parse_fixture(SNAPSHOT);
    let prompts: Vec<_> = of_kind(&p, EventKind::SystemPrompt)
        .into_iter()
        .filter(|e| !e.sidechain())
        .collect();
    let titles: Vec<_> = prompts.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["System prompt", "System prompt update"]);
    assert_eq!(prompts[1].preview, "cli prefix · tools (1)");
    assert!(!prompts[1].body.contains("You are an interactive agent."));
}

#[test]
fn subagent_snapshots_track_their_own_prompt() {
    let p = parse_fixture(SNAPSHOT);
    let side: Vec<_> = of_kind(&p, EventKind::SystemPrompt)
        .into_iter()
        .filter(|e| e.sidechain())
        .collect();
    let titles: Vec<_> = side.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["System prompt"]);
    assert!(side[0].body.contains("You are a search subagent."));
}

#[test]
fn rendered_text_wins_over_the_structured_payload() {
    let p = parse_fixture(SNAPSHOT);
    let e = &p.events[0];
    assert_eq!(e.attachment_type.as_deref(), Some("instructions"));
    assert_eq!(e.loaded_category, Some(LoadedCategory::ClaudeMd));
    assert!(e.body.contains("<system-reminder>"));
    assert_eq!(e.preview, "Codebase and user instructions are shown below.");
}

#[test]
fn nested_memory_is_claude_md_context() {
    let p = parse_fixture(SNAPSHOT);
    let e = &p.events[1];
    assert_eq!(e.title, "nested_memory: src/CLAUDE.md");
    assert_eq!(e.loaded_category, Some(LoadedCategory::ClaudeMd));
    assert_eq!(e.body, "Keep files small.");
}

#[test]
fn images_show_as_markers_never_base64() {
    let p = parse_fixture(SNAPSHOT);
    let image = p
        .events
        .iter()
        .find(|e| e.attachment_type.as_deref() == Some("image"))
        .expect("image event");
    assert_eq!(image.body, "[image: image/png]");
    let result = common::first_of(&p, EventKind::ToolResult);
    assert!(!result.body.contains("iVBOR"));
    assert!(result.body.contains("[image: image/png]"));
}

#[test]
fn the_pre_turn_prompt_stays_inside_the_system_floor() {
    let p = parse_fixture(SNAPSHOT);
    let lean = analyze_default(without_prompt(&p));
    let a = analyze_default(p);
    assert_eq!(a.snapshots[0].slices.system_tools, a.system_overhead_tokens);
    assert_eq!(a.snapshots[0].slices.other, 0.0);
    assert_eq!(a.system_overhead_tokens, lean.system_overhead_tokens);
}

// prompt-origin

const ORIGIN: &str = "sessions-claude/prompt-origin.jsonl";

#[test]
fn each_user_line_is_classified_by_its_writer() {
    use PromptOrigin::{Command, Harness, Human, Scheduled, Sdk, TaskNotification};
    let p = parse_fixture(ORIGIN);
    let got: Vec<_> = p
        .events
        .iter()
        .map(|e| (e.prompt_origin, e.prompt_source.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            (Some(Human), Some("typed")),
            (Some(Sdk), Some("sdk")),
            (Some(TaskNotification), Some("system")),
            (Some(Scheduled), Some("system")),
            (Some(Harness), None),
            (Some(Command), None),
            (Some(TaskNotification), None),
            (Some(Harness), None),
            (Some(Harness), None),
        ]
    );
}

#[test]
fn harness_prompts_are_titled_after_their_source() {
    let p = parse_fixture(ORIGIN);
    let e = &p.events;
    assert_eq!(e[2].title, "Task notification (completed)");
    assert_eq!(e[2].preview, "Background command \"bun test\" completed");
    assert_eq!(e[3].title, "Scheduled prompt");
    assert_eq!(e[4].title, "Harness message");
    assert_eq!(e[5].title, "Command /compact");
    assert_eq!(e[5].preview, "/compact keep the plan");
    assert_eq!(e[6].title, "Task notification (failed)");
    assert_eq!(e[7].title, "Command output");
}

#[test]
fn a_command_counts_once() {
    let p = parse_fixture(ORIGIN);
    let n = p
        .events
        .iter()
        .filter(|e| e.prompt_origin == Some(PromptOrigin::Command))
        .count();
    assert_eq!(n, 1);
}

#[test]
fn only_prompts_a_person_sent_count_as_user_messages() {
    let p = parse_fixture(ORIGIN);
    assert_eq!(p.events.iter().filter(|e| e.is_user_authored()).count(), 3);
    assert_eq!(analyze_default(p).user_message_count, 3);
}

#[test]
fn non_user_prompts_badge_as_their_origin() {
    let p = parse_fixture(ORIGIN);
    let labels: Vec<_> = p.events.iter().map(event_badge_label).collect();
    assert_eq!(
        labels,
        [
            "user-prompt",
            "user-prompt",
            "task notification",
            "scheduled",
            "harness",
            "command",
            "task notification",
            "harness",
            "harness",
        ]
    );
}

#[test]
fn an_event_without_an_origin_counts_as_the_users() {
    let prompt = TimelineEvent::new(EventShape::Prompt, 0, EventKind::UserPrompt);
    assert!(prompt.is_user_authored());
    let text = TimelineEvent::new(EventShape::AssistantBlock, 0, EventKind::AssistantText);
    assert!(!text.is_user_authored());
}

// measure (Claude)

fn measured() -> ParsedSession {
    parse_fixture("sessions-claude/measured-image.jsonl")
}

fn at(p: &ParsedSession, index: usize) -> Vec<&TimelineEvent> {
    p.events.iter().filter(|e| e.index == index).collect()
}

#[test]
fn a_lone_screenshot_result_gets_the_whole_step() {
    let p = measured();
    let shot = at(&p, 2)[0];
    assert_eq!(shot.tokens_measured, Some(1579.0));
    assert!(shot.tokens_est < 20.0);
}

#[test]
fn a_prompt_with_an_image_splits_the_step_image_weighted() {
    let p = measured();
    let both = at(&p, 4);
    let (text, image) = (both[0], both[1]);
    let m = |e: &TimelineEvent| e.tokens_measured.unwrap_or(0.0);
    assert_eq!(m(text) + m(image), 1614.0);
    assert!(m(image) > 1500.0);
    assert!(m(text) < 50.0);
}

#[test]
fn a_step_with_subagent_lines_keeps_the_estimate() {
    let p = measured();
    let r = at(&p, 8)[0];
    assert_eq!(r.kind, EventKind::ToolResult);
    assert_eq!(r.tokens_measured, None);
}

#[test]
fn steps_across_compaction_or_negative_delta_keep_estimates() {
    let p = measured();
    assert_eq!(at(&p, 10)[0].tokens_measured, None);
    assert_eq!(at(&p, 12)[0].tokens_measured, None);
}

#[test]
fn model_output_never_gets_a_measured_size() {
    let p = measured();
    assert!(
        p.events
            .iter()
            .filter(|e| e.request_id.is_some())
            .all(|e| e.tokens_measured.is_none())
    );
}

#[test]
fn the_budget_uses_measured_sizes() {
    let p = measured();
    let mut stripped = p.clone();
    for e in &mut stripped.events {
        e.tokens_measured = None;
    }
    let unattributed = |s: ParsedSession| {
        analyze_default(s)
            .snapshots
            .iter()
            .find(|x| x.turn_index == 2)
            .map_or(0.0, |x| x.slices.unattributed)
    };
    assert!(unattributed(stripped) > 3000.0);
    assert_eq!(unattributed(p), 0.0);
}

// measureTokens rules

fn ev(index: usize, kind: EventKind, tokens_est: f64) -> TimelineEvent {
    TimelineEvent::new(EventShape::ToolResult, index, kind).text(
        String::new(),
        String::new(),
        "x".repeat(400),
        tokens_est,
    )
}

fn result(index: usize) -> TimelineEvent {
    ev(index, EventKind::ToolResult, 100.0)
}

fn call(index: usize, req: &str) -> TimelineEvent {
    let mut e = ev(index, EventKind::AssistantText, 5.0);
    e.request_id = Some(req.to_owned());
    e
}

fn turn(req: &str, ctx: f64, output: f64) -> Turn {
    Turn {
        shape: TurnShape::Claude,
        request_id: req.to_owned(),
        model: "m".to_owned(),
        context_tokens: ctx,
        input_tokens: ctx,
        cache_read_tokens: 0.0,
        cache_creation_tokens: 0.0,
        output_tokens: output,
        cache_creation_1h_tokens: None,
        cache_creation_5m_tokens: None,
        event_indexes: Vec::new(),
        ts: None,
        thinking_tokens: None,
        web_search_requests: None,
        web_fetch_requests: None,
        effort: None,
        cost_usd: None,
        appended: Vec::new(),
    }
}

fn measure(events: Vec<TimelineEvent>, turns: Vec<Turn>) -> ParsedSession {
    let mut p = ParsedSession {
        provider: Provider::Pi,
        session_id: "s".to_owned(),
        path: "p".to_owned(),
        models: Vec::new(),
        events,
        turns,
        compaction_indexes: Vec::new(),
        subagents: Vec::new(),
        cwd: None,
        git_branch: None,
        title: None,
        version: None,
        started_at: None,
        ended_at: None,
        native_context_window: None,
        unknown_record_types: None,
        unknown_system_subtypes: None,
        sidechain_cost: None::<TurnsCost>,
        native_cost_usd: None,
    };
    measure_tokens(&mut p);
    p
}

#[test]
fn splits_by_estimate_and_keeps_the_sum_exact() {
    let p = measure(
        vec![
            call(0, "a"),
            ev(1, EventKind::ToolResult, 100.0),
            ev(2, EventKind::ToolResult, 200.0),
            call(3, "b"),
        ],
        vec![turn("a", 1000.0, 10.0), turn("b", 1611.0, 0.0)],
    );
    assert_eq!(at(&p, 1)[0].tokens_measured, Some(200.0));
    assert_eq!(at(&p, 2)[0].tokens_measured, Some(401.0));
}

#[test]
fn a_small_delta_over_equal_events_never_goes_negative() {
    let mut events = vec![call(0, "a")];
    events.extend((1..=6).map(result));
    events.push(call(7, "b"));
    let p = measure(events, vec![turn("a", 100.0, 10.0), turn("b", 113.0, 0.0)]);
    let shares: Vec<_> = (1..=6).map(|i| at(&p, i)[0].tokens_measured).collect();
    assert_eq!(
        shares,
        [
            Some(1.0),
            Some(1.0),
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(0.0)
        ]
    );
}

#[test]
fn a_context_edit_inside_the_step_skips_it() {
    let mut edited = result(1);
    edited.context_edit = Some(ContextEdit::Removed {
        at_index: 2.0,
        steps: None,
    });
    let p = measure(
        vec![call(0, "a"), edited, result(2), call(3, "b")],
        vec![turn("a", 1000.0, 0.0), turn("b", 1300.0, 0.0)],
    );
    assert!(p.events.iter().all(|e| e.tokens_measured.is_none()));
}

#[test]
fn a_system_prompt_change_inside_the_step_skips_it() {
    let p = measure(
        vec![
            call(0, "a"),
            ev(1, EventKind::SystemPrompt, 100.0),
            result(2),
            call(3, "b"),
        ],
        vec![turn("a", 1000.0, 0.0), turn("b", 1300.0, 0.0)],
    );
    assert_eq!(at(&p, 2)[0].tokens_measured, None);
}

// compaction-buckets

fn compaction_rows() -> Vec<Value> {
    let usage = |ctx: u32| json!({"input_tokens":10,"cache_read_input_tokens":ctx - 10,"cache_creation_input_tokens":0,"output_tokens":20});
    let listings = [
        json!({"type":"skill_listing","content":"- demo-skill: does a demo thing\n".repeat(25),"skillCount":25}),
        json!({"type":"agent_listing_delta","addedLines":["- helper: a demo agent","- checker: a demo agent"]}),
        json!({"type":"deferred_tools_delta","addedNames":["DemoToolA","DemoToolB","DemoToolC"]}),
        json!({"type":"mcp_instructions_delta","addedBlocks":["## demo-server\nUse the demo server for demo things."]}),
    ];
    let memory = json!({"type":"nested_memory","path":"/repo/sub/CLAUDE.md",
        "content":{"content":"# Demo rules\nKeep functions small.\n".repeat(10)}});
    let attach = |a: &Value, n: usize| json!({"type":"attachment","uuid":format!("a{n}"),"timestamp":"2026-11-01T09:00:00.000Z","attachment":a});
    let assistant = |req: &str, ctx: u32| {
        json!({"type":"assistant","uuid":format!("t-{req}"),"timestamp":"2026-11-01T09:00:05.000Z",
            "requestId":req,"message":{"model":"claude-opus-5-5",
                "content":[{"type":"text","text":"ok"}],"usage":usage(ctx)}})
    };
    let user = |text: &str, summary: bool| {
        let mut u = json!({"type":"user","uuid":format!("u-{}", text.len()),
            "timestamp":"2026-11-01T09:00:00.000Z","sessionId":"s1",
            "message":{"role":"user","content":text}});
        if summary {
            u["isCompactSummary"] = json!(true);
        }
        u
    };
    let mut rows = vec![user("start the demo", false)];
    rows.extend(
        listings
            .iter()
            .chain(std::iter::once(&memory))
            .enumerate()
            .map(|(i, a)| attach(a, i)),
    );
    rows.push(assistant("r1", 50_000));
    rows.push(
        json!({"type":"system","subtype":"compact_boundary","content":"Conversation compacted"}),
    );
    rows.push(user("Summary of the demo so far.", true));
    rows.extend(listings.iter().enumerate().map(|(i, a)| attach(a, i + 10)));
    rows.push(user("continue", false));
    rows.push(assistant("r2", 60_000));
    rows
}

#[test]
fn listings_are_counted_once_after_a_compaction() {
    let a = analyze_default(parse_rows(&compaction_rows()));
    assert_eq!(a.compaction_turns, [1]);
    let (before, after) = (&a.snapshots[0], &a.snapshots[1]);
    assert!(before.slices.listings > 0.0);
    assert_eq!(after.slices.listings, before.slices.listings);
}

#[test]
fn memory_loaded_before_a_compaction_drops_to_zero() {
    let a = analyze_default(parse_rows(&compaction_rows()));
    let (before, after) = (&a.snapshots[0], &a.snapshots[1]);
    assert!(before.slices.memory > 0.0);
    assert_eq!(after.slices.memory, 0.0);
}
