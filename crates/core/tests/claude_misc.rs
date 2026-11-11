//! Small helpers and defensive parsing. Ported from `first-line`,
//! `defensive`, `images`, `skip-system-prompt`, `labels` and `tool-schema`
//! TS tests.
#![allow(clippy::float_cmp)]

mod common;

use common::parse_text;
use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::units::first_line;
use peektrace_core::js::value::JsValue;
use peektrace_core::sessions::images::image_at;
use peektrace_core::sessions::jsonl::{line_at, parse_rows as parse_jsonl};
use peektrace_core::sessions::labels::event_badge_label;
use peektrace_core::sessions::model::{
    EventKind, EventShape, ImageRef, ParsedSession, TimelineEvent, Turn,
};
use peektrace_core::sessions::system_prompt::{render_tool, tool_wire_text};
use serde_json::{Value, json};

// first-line

/// The split-based version, kept as an oracle.
fn old_first_line(s: &str, cap: usize) -> String {
    let line = s.split('\n').find(|l| !l.trim().is_empty()).unwrap_or("");
    let t = line.trim();
    if t.chars().count() > cap {
        format!("{}…", t.chars().take(cap).collect::<String>())
    } else {
        t.to_owned()
    }
}

fn samples() -> Vec<String> {
    let mut s: Vec<String> = [
        "",
        "\n",
        "\n\n",
        "   \n\t\n",
        "\n\n  hello world  \nsecond",
        "  \n \t \nfirst\nsecond",
        "line one\r\nline two",
        "\r\n\r\nafter crlf\r\n",
        "no newline at all",
        "only at the end\n",
        "\n\n\nthe line is last",
        "a\n",
    ]
    .map(str::to_owned)
    .to_vec();
    s.push("x".repeat(300));
    s.push(format!("\n  {}  \nz", "y".repeat(300)));
    s
}

#[test]
fn first_line_matches_the_split_version() {
    for s in samples() {
        assert_eq!(first_line(&s, 200), old_first_line(&s, 200), "{s:?}");
    }
}

#[test]
fn first_line_honours_a_custom_cap() {
    for s in samples() {
        for cap in [0, 1, 5, 10] {
            assert_eq!(first_line(&s, cap), old_first_line(&s, cap), "{s:?} {cap}");
        }
    }
    assert_eq!(first_line("\n abcdefgh \n", 3), "abc…");
}

// defensive

fn broken_transcript() -> String {
    let good = json!({
        "type":"assistant","requestId":"req-1","timestamp":"2026-06-01T10:00:05.000Z",
        "message":{"model":"claude-opus-4",
            "usage":{"input_tokens":100,"cache_read_input_tokens":0,"output_tokens":50},
            "content":[{"type":"text","text":"hello"}]}
    })
    .to_string();
    [
        r#"{"type":"user","message":{"content":"first"}}"#,
        "this is not json at all",
        r#"{"type":"assistant","message":{"#,
        "",
        "   ",
        r#"{"type":"user""#,
        &good,
        r#"{"type":"unknown-future-kind","weird":true}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text""#,
    ]
    .join("\n")
}

#[test]
fn parse_jsonl_skips_malformed_lines() {
    let rows = parse_jsonl(&broken_transcript());
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.as_obj().is_some()));
}

#[test]
fn parse_never_fails_on_a_broken_transcript() {
    let p = parse_text(&broken_transcript());
    assert_eq!(p.turns.len(), 1);
    assert!(p.models.iter().any(|m| m == "claude-opus-4"));
    assert!(p.events.iter().any(|e| e.kind == EventKind::UserPrompt));
    assert!(p.events.iter().any(|e| e.kind == EventKind::AssistantText));
}

// images

const IMAGE_FIXTURE: &str = "sessions-claude/measured-image.jsonl";

fn js(src: &str) -> JsValue {
    JsValue::parse(src).expect("valid JSON")
}

#[test]
fn line_at_numbers_lines_like_parse_rows() {
    let text = common::read_fixture(IMAGE_FIXTURE);
    for (n, row) in parse_jsonl(&text).iter().enumerate() {
        assert_eq!(line_at(&text, n).as_ref(), Some(row), "line {n}");
    }
    let gappy = "{\"a\":1}\n\n  \n{\"b\":2}\n{\"c\":";
    assert_eq!(line_at(gappy, 1), Some(js(r#"{"b":2}"#)));
    assert_eq!(line_at(gappy, 2), None);
    assert_eq!(line_at("{\"a\":1}\n{bad}\n", 1), None);
}

#[test]
fn pasted_image_and_screenshot_each_carry_a_ref() {
    let p = common::parse_fixture(IMAGE_FIXTURE);
    let with: Vec<_> = p.events.iter().filter(|e| e.images.is_some()).collect();
    let got: Vec<_> = with
        .iter()
        .map(|e| (e.kind, e.index, e.images.clone().unwrap_or_default()))
        .collect();
    let png = |block, item| ImageRef {
        block,
        media_type: "image/png".to_owned(),
        item,
    };
    assert_eq!(
        got,
        [
            (EventKind::ToolResult, 2, vec![png(0, Some(0))]),
            (EventKind::Attachment, 4, vec![png(1, None)]),
        ]
    );
    assert!(with.iter().all(|e| !e.body.contains("iVBORw0KGgo")));
}

#[test]
fn a_ref_resolves_to_the_base64_on_its_line() {
    let lines = parse_jsonl(&common::read_fixture(IMAGE_FIXTURE));
    let pasted = image_at(&lines[4], 1, None).expect("image");
    assert_eq!(pasted.media_type, "image/png");
    let shot = image_at(&lines[2], 0, Some(0)).expect("image");
    assert!(shot.data.starts_with("iVBORw0KGgo"));
}

#[test]
fn a_ref_to_anything_but_a_base64_image_resolves_to_nothing() {
    let lines = parse_jsonl(&common::read_fixture(IMAGE_FIXTURE));
    assert_eq!(image_at(&lines[4], 0, None), None);
    assert_eq!(image_at(&lines[2], 0, None), None);
    let svg = js(
        r#"{"message":{"content":[{"type":"image","source":{"type":"base64","media_type":"image/svg+xml","data":"PHN2Zz4="}}]}}"#,
    );
    assert_eq!(image_at(&svg, 0, None), None);
    let url = js(
        r#"{"message":{"content":[{"type":"image","source":{"type":"url","url":"https://x/y.png"}}]}}"#,
    );
    assert_eq!(image_at(&url, 0, None), None);
}

// skip-system-prompt

fn snapshot(at: &str, tools: &[&str]) -> Value {
    let tools: Vec<Value> = tools
        .iter()
        .map(|name| {
            json!({"name": name, "description": format!("Runs {name}."), "schema": {
                "name": name,
                "input_schema": {"type":"object",
                    "properties":{"arg":{"type":"string","description":"An argument"}},
                    "required":["arg"]}
            }})
        })
        .collect();
    json!({"type":"attachment","timestamp": at,"cwd":"/work/proj","attachment":{
        "type":"prompt_snapshot",
        "systemPrompt":["You are a test agent.","# Env\n - cwd: /work/proj"],
        "tools": tools
    }})
}

fn skip_text() -> String {
    let rows = [
        snapshot("2026-08-01T10:00:00.000Z", &["Alpha"]),
        json!({"type":"user","cwd":"/work/proj","timestamp":"2026-08-01T10:00:01.000Z",
            "message":{"role":"user","content":"do the task"}}),
        json!({"type":"assistant","requestId":"req_1","timestamp":"2026-08-01T10:00:02.000Z",
            "message":{"model":"claude-test",
                "usage":{"input_tokens":5,"cache_read_input_tokens":900,"cache_creation_input_tokens":100,"output_tokens":10},
                "content":[{"type":"text","text":"on it"},
                    {"type":"tool_use","id":"toolu_1","name":"Alpha","input":{"arg":"a"}}]}}),
        json!({"type":"user","timestamp":"2026-08-01T10:00:03.000Z","message":{"role":"user",
            "content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"done"}]}}),
        snapshot("2026-08-01T10:00:04.000Z", &["Alpha", "Beta"]),
        json!({"type":"user","timestamp":"2026-08-01T10:00:05.000Z","message":{"role":"user","content":"and again"}}),
    ];
    format!("{}\n", common::jsonl(&rows))
}

fn parse_skip(skip: bool) -> ParsedSession {
    let text = skip_text();
    parse_claude_session(&ParseArgs {
        text: &text,
        path: "/fixture/proj/sess-1.jsonl",
        session_id: "sess-1",
        skip_system_prompt: skip,
        ..ParseArgs::default()
    })
}

#[test]
fn by_default_both_snapshots_render_with_tools() {
    let p = parse_skip(false);
    let prompts = common::of_kind(&p, EventKind::SystemPrompt);
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].body.contains("Alpha"));
    assert!(prompts[1].body.contains("Beta"));
}

#[test]
fn with_the_flag_no_system_prompt_event_is_built() {
    let p = parse_skip(true);
    assert!(p.events.iter().all(|e| e.kind != EventKind::SystemPrompt));
    assert_ne!(p.events.len(), 0);
}

#[test]
fn skipping_leaves_the_rest_unchanged() {
    let full = parse_skip(false);
    let lean = parse_skip(true);
    let shape = |e: &TimelineEvent| TimelineEvent {
        tokens_measured: None,
        ..e.clone()
    };
    let lean_events: Vec<_> = lean.events.iter().map(shape).collect();
    let full_events: Vec<_> = full
        .events
        .iter()
        .filter(|e| e.kind != EventKind::SystemPrompt)
        .map(shape)
        .collect();
    assert_eq!(lean_events, full_events);
    let turns_of = |p: &ParsedSession| {
        p.turns
            .iter()
            .map(|t| {
                let lines: Vec<_> = t
                    .event_indexes
                    .iter()
                    .map(|&i| usize::try_from(i).ok().map(|i| p.events[i].index))
                    .collect();
                (
                    Turn {
                        event_indexes: Vec::new(),
                        ..t.clone()
                    },
                    lines,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(turns_of(&lean), turns_of(&full));
    assert_eq!(lean.cwd, full.cwd);
    assert_eq!(lean.models, full.models);
}

// labels

fn attachment(ty: Option<&str>) -> TimelineEvent {
    let mut e = TimelineEvent::new(EventShape::Attachment, 3, EventKind::Attachment).text(
        "attachment".to_owned(),
        "…".to_owned(),
        "…".to_owned(),
        12.0,
    );
    e.attachment_type = ty.map(str::to_owned);
    e
}

#[test]
fn an_attachment_badges_as_its_type() {
    assert_eq!(
        event_badge_label(&attachment(Some("skill_listing"))),
        "skill listing"
    );
    assert_eq!(
        event_badge_label(&attachment(Some("opened_file_in_ide"))),
        "opened file in ide"
    );
}

#[test]
fn an_unseen_type_still_reads_as_words() {
    assert_eq!(
        event_badge_label(&attachment(Some("some_future_thing"))),
        "some future thing"
    );
}

#[test]
fn a_missing_or_empty_type_falls_back_to_the_kind() {
    for ty in [None, Some(""), Some("   ")] {
        assert_eq!(event_badge_label(&attachment(ty)), "attachment");
    }
}

#[test]
fn a_tool_call_badges_as_its_tool() {
    let mut call = TimelineEvent::new(EventShape::AssistantBlock, 0, EventKind::ToolCall).text(
        "Bash".to_owned(),
        "ls".to_owned(),
        "{}".to_owned(),
        4.0,
    );
    call.tool_name = Some("Bash".to_owned());
    assert_eq!(event_badge_label(&call), "Bash");
}

#[test]
fn an_event_with_neither_badges_as_its_kind() {
    let e = TimelineEvent::new(EventShape::AssistantBlock, 1, EventKind::AssistantText).text(
        "Assistant".to_owned(),
        "hi".to_owned(),
        "hi".to_owned(),
        1.0,
    );
    assert_eq!(event_badge_label(&e), "assistant-text");
}

// tool-schema

const BASH: &str = r#"{"name":"Bash","description":"Executes a bash command.","schema":{"name":"Bash","description":"Executes a bash command.","input_schema":{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"command":{"type":"string","description":"The command to run"},"timeout":{"type":"number","maximum":600000},"mode":{"enum":["fg","bg"],"description":"Where it runs\nand why"},"files":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},"required":["command"]}}}"#;

#[test]
fn a_claude_code_tool_shows_once_with_parameters() {
    let params = [
        "**Parameters**",
        "",
        "- `command` *string, required* — The command to run",
        "- `timeout` *number, maximum: 600000*",
        "- `mode` *\"fg\" | \"bg\"* — Where it runs\n  and why",
        "- `files` *object[]*",
        "  - `path` *string, required*",
    ]
    .join("\n");
    let want = ["### Bash", "Executes a bash command.", &params].join("\n\n");
    assert_eq!(render_tool(&js(BASH), 3), want);
}

#[test]
fn a_namespace_lists_its_tools_one_level_down() {
    let ns = js(
        r#"{"type":"namespace","name":"app","description":"App tools","tools":[{"name":"open_panel","description":"Open a panel"}]}"#,
    );
    assert_eq!(
        render_tool(&ns, 3),
        "### app\n\nApp tools\n\n#### open_panel\n\nOpen a panel"
    );
}

#[test]
fn unknown_keys_stay_visible_as_json() {
    let t = js(r#"{"name":"x","cache_control":{"type":"ephemeral"}}"#);
    assert_eq!(
        render_tool(&t, 3),
        "### x\n\n```json\n{\n  \"cache_control\": {\n    \"type\": \"ephemeral\"\n  }\n}\n```"
    );
}

#[test]
fn the_wire_text_is_the_api_copy() {
    let wire: Value = serde_json::from_str(&tool_wire_text(&js(BASH))).expect("json");
    let bash: Value = serde_json::from_str(BASH).expect("json");
    assert_eq!(wire, bash["schema"]);
}
