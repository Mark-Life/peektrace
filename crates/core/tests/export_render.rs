//! Markdown, JSON and HTML exports of a synthetic parsed session. Ported
//! from the renderer cases of the `export` TS test.

use peektrace_core::export::{
    ExportFormat, ExportOptions, fence_for, render_export, render_html, render_json,
    render_markdown,
};
use peektrace_core::js::units;
use peektrace_core::sessions::labels::event_badge_label;
use peektrace_core::sessions::model::{
    ContextEdit, EventKind, EventShape, ParsedSession, PromptOrigin, Provider, SubagentRef,
    TimelineEvent, TurnsCost,
};
use regex::Regex;
use serde_json::Value;

const TRANSCRIPT_PATH: &str = "/home/demo/.claude/projects/x/secret-transcript.jsonl";
const SUBAGENT_PATH: &str = "/home/demo/.claude/projects/x/subagents/agent-a.jsonl";
const XSS: &str = r#"<script>alert(1)</script> "onerror=alert(2) <img src=x onerror=alert(3)>"#;

fn ev(index: usize, kind: EventKind, body: &str) -> TimelineEvent {
    TimelineEvent::new(EventShape::AssistantBlock, index, kind).text(
        kind.as_str().to_owned(),
        units::slice(body, 0, 20),
        body.to_owned(),
        10.0,
    )
}

fn human(mut e: TimelineEvent) -> TimelineEvent {
    e.prompt_origin = Some(PromptOrigin::Human);
    e
}

fn session() -> ParsedSession {
    let mut thinking = ev(1, EventKind::AssistantThinking, "");
    thinking.preview = String::new();
    let mut call = ev(
        3,
        EventKind::ToolCall,
        r#"{"file_path":"/home/demo/work/a.ts"}"#,
    );
    call.tool_name = Some("Read".to_owned());
    call.tool_use_id = Some("t1".to_owned());
    "Read /home/demo/work/a.ts".clone_into(&mut call.title);
    let mut result = ev(4, EventKind::ToolResult, "line 1\n````\nnested fence\n````");
    result.tool_use_id = Some("t1".to_owned());
    ParsedSession {
        provider: Provider::ClaudeCode,
        session_id: "sess-1".to_owned(),
        path: TRANSCRIPT_PATH.to_owned(),
        models: vec!["claude-opus-4-1".to_owned()],
        events: vec![
            human(ev(0, EventKind::UserPrompt, &format!("hello {XSS}"))),
            thinking,
            ev(
                2,
                EventKind::AssistantText,
                "sure, here is code:\n```ts\nconst a = 1\n```",
            ),
            call,
            result,
            ev(5, EventKind::Compaction, "summary of earlier work"),
            human(ev(6, EventKind::UserPrompt, "look [image: image/png]")),
        ],
        turns: Vec::new(),
        compaction_indexes: vec![5],
        subagents: vec![SubagentRef {
            id: "agent-a".to_owned(),
            path: SUBAGENT_PATH.to_owned(),
            agent_type: Some("explore".to_owned()),
            description: Some("scan /home/demo/work".to_owned()),
            tool_use_id: None,
            parent_agent_id: None,
            workflow_run_id: None,
            workflow_phase: None,
            workflow_phase_index: None,
            tool_use_id_linked: false,
            turns: 2,
            peak_context_tokens: 1234.0,
            cost: TurnsCost::default(),
            unreadable: false,
        }],
        cwd: Some("/home/demo/work".to_owned()),
        git_branch: Some("main".to_owned()),
        title: Some("Demo <session>".to_owned()),
        version: None,
        started_at: Some("2026-01-01T00:00:00.000Z".to_owned()),
        ended_at: Some("2026-01-01T00:05:00.000Z".to_owned()),
        native_context_window: None,
        unknown_record_types: None,
        unknown_system_subtypes: None,
        sidechain_cost: None,
        native_cost_usd: None,
    }
}

fn opts() -> ExportOptions {
    ExportOptions {
        exported_at: "2026-02-02T00:00:00.000Z".to_owned(),
        home: None,
        redacted: true,
    }
}

fn re(src: &str) -> Regex {
    Regex::new(src).expect("regex")
}

#[test]
fn fence_is_longer_than_any_backtick_run() {
    assert_eq!(fence_for("plain"), "```");
    assert_eq!(fence_for("a ``` b"), "````");
    assert_eq!(fence_for("a ````` b ``"), "``````");
}

// markdown

fn md() -> String {
    render_markdown(&session(), &opts())
}

#[test]
fn headings_count_exported_events_and_skip_empty_ones() {
    let md = md();
    let order: Vec<_> = re(r"(?m)^### (\d+)\.")
        .captures_iter(&md)
        .map(|c| c[1].to_owned())
        .collect();
    assert_eq!(order, ["1", "2", "3", "4", "5", "6"]);
    assert!(md.starts_with("# Demo <session>"));
}

#[test]
fn a_body_holding_a_fence_gets_a_longer_fence() {
    assert!(md().contains("`````\nline 1\n````\nnested fence\n````\n`````"));
}

#[test]
fn labels_match_the_shared_badge_labels() {
    let md = md();
    for e in session()
        .events
        .iter()
        .filter(|e| e.kind == EventKind::ToolCall)
    {
        assert!(md.contains(&format!(". {}", event_badge_label(e))));
    }
}

#[test]
fn markdown_has_no_transcript_paths_and_collapses_home() {
    let md = md();
    assert!(!md.contains("secret-transcript"));
    assert!(!md.contains("agent-a.jsonl"));
    assert!(!md.contains("/home/demo"));
    assert!(md.contains("Directory: ~/work"));
    assert!(md.contains("`agent-a`: explore, scan ~/work, 2 turns"));
}

#[test]
fn image_markers_stay_as_written() {
    assert!(md().contains("[image: image/png]"));
}

// json

fn json_out() -> Value {
    serde_json::from_str(&render_json(&session(), &opts())).expect("json")
}

#[test]
fn json_has_schema_version_and_meta() {
    let out = json_out();
    assert_eq!(out["schema"], "peektrace.session-export");
    assert_eq!(out["version"], 1);
    assert_eq!(out["redacted"], true);
    assert_eq!(out["exportedAt"], "2026-02-02T00:00:00.000Z");
    // TS builds this session with provider "claude"; Rust only has the
    // provider names parsers write.
    assert_eq!(out["session"]["provider"], "claude-code");
    assert_eq!(
        out["session"]["models"],
        serde_json::json!(["claude-opus-4-1"])
    );
}

#[test]
fn json_keeps_event_kinds_in_order_and_skips_empty_events() {
    let out = json_out();
    let kinds: Vec<_> = out["session"]["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        kinds,
        [
            "user-prompt",
            "assistant-text",
            "tool-call",
            "tool-result",
            "compaction",
            "user-prompt"
        ]
    );
}

#[test]
fn json_exports_no_paths_turns_or_images() {
    let raw = render_json(&session(), &opts());
    assert!(!raw.contains("secret-transcript"));
    assert!(!raw.contains("agent-a.jsonl"));
    let out = json_out();
    assert_eq!(out["session"].get("path"), None);
    assert_eq!(out["session"]["subagents"][0].get("path"), None);
    assert_eq!(out["session"].get("turns"), None);
    let mut keys: Vec<_> = out["session"]["subagents"][0]
        .as_object()
        .expect("subagent")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "agentType",
            "description",
            "id",
            "peakContextTokens",
            "turns"
        ]
    );
}

// markdown fences and notes

fn with_events(events: Vec<TimelineEvent>) -> String {
    render_markdown(
        &ParsedSession {
            events,
            ..session()
        },
        &opts(),
    )
}

#[test]
fn an_unclosed_fence_does_not_swallow_later_turns() {
    let md = with_events(vec![
        ev(0, EventKind::UserPrompt, "hi"),
        ev(1, EventKind::AssistantText, "```ts\nconst x = 1"),
        ev(2, EventKind::UserPrompt, "second prompt"),
    ]);
    let (head, tail) = md.split_once("### 3. User").expect("third heading");
    assert_eq!(re("(?m)^```").find_iter(head).count() % 2, 0);
    assert!(tail.contains("second prompt"));
}

#[test]
fn an_inline_code_span_starting_a_line_is_not_a_fence() {
    let md = with_events(vec![
        ev(
            0,
            EventKind::AssistantText,
            "```inline``` is how you write a code span.\nDone.",
        ),
        ev(1, EventKind::UserPrompt, "SECOND visible?"),
    ]);
    assert!(!re("(?m)^```$").is_match(&md), "{md}");
}

#[test]
fn an_unclosed_html_comment_does_not_swallow_later_turns() {
    let md = with_events(vec![
        ev(0, EventKind::AssistantText, "Add this:\n\n<!-- TODO"),
        ev(1, EventKind::UserPrompt, "THIRD"),
    ]);
    assert!(md.contains("<!-- TODO\n-->\n\n### 2. User"), "{md}");
    let closed = with_events(vec![ev(0, EventKind::AssistantText, "<!-- a -->\nx")]);
    assert!(!closed.contains("\n-->\n\n"));
}

#[test]
fn headings_number_events_by_position() {
    let md = with_events(vec![
        ev(5, EventKind::UserPrompt, "a"),
        ev(5, EventKind::AssistantText, "b"),
        ev(9, EventKind::AssistantText, "c"),
    ]);
    assert!(md.contains("### 1. User"));
    assert!(md.contains("### 2. Assistant"));
    assert!(md.contains("### 3. Assistant"));
}

#[test]
fn balanced_fences_are_left_alone() {
    let body = "a\n```ts\nx\n```\nb";
    let md = with_events(vec![ev(0, EventKind::AssistantText, body)]);
    assert!(md.contains(&format!("{body}\n")));
}

#[test]
fn context_edit_notes_show_on_chat_events() {
    let mut e = ev(0, EventKind::AssistantText, "x");
    e.context_edit = Some(ContextEdit::Removed {
        at_index: 9.0,
        steps: None,
    });
    assert!(with_events(vec![e]).contains("Dropped"));
}

// html

fn html() -> String {
    render_html(&session(), &opts())
}

#[test]
fn html_escapes_content_with_no_script_or_handlers() {
    let html = html();
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("&quot;onerror=alert(2)"));
    assert!(!html.contains("<script"));
    assert!(!re(r"<[^>]+[ \t\n\r\f]on[A-Za-z0-9_]+=").is_match(&html));
    assert!(!html.contains("<img"));
}

#[test]
fn html_chat_turns_are_headings() {
    let html = html();
    assert!(html.contains("<h3 class=\"who\">User"));
    assert!(html.contains("<h3 class=\"who\">Assistant"));
}

#[test]
fn html_escapes_the_title() {
    assert!(html().contains("<title>Demo &lt;session&gt;</title>"));
}

#[test]
fn html_loads_nothing_external_and_sets_csp() {
    let html = html();
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("name=\"referrer\" content=\"no-referrer\""));
    assert!(!re(r"<[^>]*[ \t\n\r\f](?:src|href)=").is_match(&html));
    assert!(!re("https?:").is_match(&html));
}

#[test]
fn html_has_no_transcript_paths() {
    let html = html();
    assert!(!html.contains("secret-transcript"));
    assert!(!html.contains("agent-a.jsonl"));
    assert!(!html.contains("/home/demo"));
}

#[test]
fn html_error_color_is_a_variable_with_a_dark_value() {
    let html = html();
    assert!(html.contains(".err summary{color:var(--err)}"));
    assert!(re(r"prefers-color-scheme:dark[^}]*--err:").is_match(&html));
}

#[test]
fn not_redacted_keeps_home_paths_in_every_format() {
    let raw = ExportOptions {
        redacted: false,
        ..opts()
    };
    for format in ExportFormat::ALL {
        let out = render_export(format, &session(), &raw);
        assert!(out.contains("/home/demo/work"), "{}", format.as_str());
        assert!(!out.contains("secret-transcript"), "{}", format.as_str());
    }
}
