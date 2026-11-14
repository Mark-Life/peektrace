//! Ports of `apps/inspector/test/tool-event.test.ts` that need the real
//! parser: the shared Claude fixture and the built outcomes transcript, both
//! run through core's parse, analyze and redact, then decoded into view types.

use std::path::PathBuf;

use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::Utc;
use peektrace_core::sessions::analyze::{AnalyzeOptions, analyze};
use peektrace_core::sessions::redact::redact_session;
use peektrace_core::sessions::write::analyzed_json_with;
use peektrace_protocol::view::{AnalyzedSession, EventKind, EventMeta, mend_surrogates};
use peektrace_ui::model::diff::RowKind;
use peektrace_ui::model::tools::{
    Body, ToolPane, ToolState, call_view, index_tools, result_pane, tool_name_of, tool_state,
};
use serde_json::json;

const FIXTURE_ID: &str = "11111111-1111-4111-8111-111111111111";
const SECRET: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

fn session_from(text: &str, session_id: &str, redact: bool) -> AnalyzedSession {
    let path = format!("/tmp/{session_id}.jsonl");
    let parsed = parse_claude_session(&ParseArgs {
        text,
        path: &path,
        session_id,
        ..ParseArgs::default()
    });
    let mut a = analyze(
        parsed,
        AnalyzeOptions {
            dumb_zone_fraction: None,
            on_disk_context_files: Vec::new(),
            window: None,
            local: &Utc,
        },
    );
    if redact {
        redact_session(&mut a);
    }
    serde_json::from_str(&mend_surrogates(&analyzed_json_with(&a, false)))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// The shared Claude fixture: one `Read` call, one result carrying a secret.
fn golden(redact: bool) -> AnalyzedSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../packages/core/test/fixtures/sessions/projects/-Users-demo-proj/{FIXTURE_ID}.jsonl"
    ));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
    session_from(&text, FIXTURE_ID, redact)
}

/// A transcript built to hit the outcomes the golden fixture has no example of.
fn outcomes() -> AnalyzedSession {
    let lines = [
        json!({
            "type": "assistant",
            "requestId": "req-1",
            "message": {
                "model": "claude-opus-4",
                "usage": {
                    "input_tokens": 10,
                    "cache_read_input_tokens": 0,
                    "cache_creation_input_tokens": 0,
                    "output_tokens": 10
                },
                "content": [
                    {"type": "tool_use", "id": "call-ok", "name": "Bash", "input": {
                        "command": "bun test\necho done",
                        "description": "Run the suite",
                        "timeout": 120_000
                    }},
                    {"type": "tool_use", "id": "call-bad", "name": "Grep", "input": {"pattern": "TODO"}},
                    {"type": "tool_use", "id": "call-open", "name": "Write", "input": {"file_path": "/a.ts"}},
                    {"type": "tool_use", "id": "call-edit", "name": "Edit", "input": {
                        "file_path": "/src/a.ts",
                        "old_string": "const a = 1;\nconst b = 2;",
                        "new_string": "const a = 1;\nconst b = 3;",
                        "replace_all": false
                    }},
                    {"type": "tool_use", "id": "call-multi", "name": "MultiEdit", "input": {
                        "file_path": "/docs/b.md",
                        "edits": [
                            {"old_string": "# One", "new_string": "# Uno"},
                            {"old_string": "# Two", "new_string": "# Dos"}
                        ]
                    }}
                ]
            }
        }),
        json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "call-ok", "content": "ok"},
                {"type": "tool_result", "tool_use_id": "call-bad", "is_error": true, "content": "boom"}
            ]}
        }),
    ];
    let text: Vec<String> = lines.iter().map(ToString::to_string).collect();
    session_from(&text.join("\n"), "outcomes", false)
}

fn event_at(a: &AnalyzedSession, use_id: &str, kind: EventKind) -> usize {
    a.events
        .iter()
        .position(|e| e.tool_use_id.as_deref() == Some(use_id) && e.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} for {use_id}"))
}

fn code(p: &ToolPane) -> &str {
    match p {
        ToolPane::Code { code, .. } => code,
        _ => "",
    }
}

fn json_of(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("{e}: {s}"))
}

#[test]
fn a_result_borrows_the_tool_name_its_event_never_records() {
    let a = golden(false);
    let ix = index_tools(&a.events);
    let result = &a.events[event_at(&a, "tool-1", EventKind::ToolResult)];
    assert_eq!(result.tool_name, None);
    assert_eq!(tool_name_of(&a.events, result, &ix), "Read");
}

#[test]
fn an_unpaired_event_still_names_something_renderable() {
    let a = outcomes();
    let ix = index_tools(&a.events);
    let mut orphan = a.events[event_at(&a, "call-ok", EventKind::ToolResult)].clone();
    orphan.meta = EventMeta {
        title: "tool_result".into(),
        tool_use_id: Some("nothing-matches-this".into()),
        tool_name: None,
        ..orphan.meta
    };
    assert_eq!(tool_name_of(&a.events, &orphan, &ix), "tool");
}

#[test]
fn outcome_follows_the_result_not_the_call() {
    let a = outcomes();
    let ix = index_tools(&a.events);
    let state = |id, k| tool_state(&a.events, &a.events[event_at(&a, id, k)], &ix);
    assert_eq!(state("call-ok", EventKind::ToolCall), ToolState::Completed);
    assert_eq!(state("call-bad", EventKind::ToolCall), ToolState::Error);
    assert_eq!(state("call-bad", EventKind::ToolResult), ToolState::Error);
}

#[test]
fn a_call_whose_result_never_arrived_is_unanswered() {
    let a = outcomes();
    let ix = index_tools(&a.events);
    let open = &a.events[event_at(&a, "call-open", EventKind::ToolCall)];
    assert_eq!(tool_state(&a.events, open, &ix), ToolState::Unanswered);
}

fn call_of(id: &str) -> peektrace_ui::model::tools::CallView {
    let a = outcomes();
    call_view(&a.events[event_at(&a, id, EventKind::ToolCall)])
}

#[test]
fn unwraps_the_payload_arg_so_its_newlines_survive() {
    let v = call_of("call-ok");
    assert_eq!(
        v.panes[0],
        ToolPane::Code {
            label: "command".into(),
            code: "bun test\necho done".into(),
            language: "bash"
        }
    );
}

#[test]
fn keeps_every_other_argument_in_a_pane_of_its_own() {
    let v = call_of("call-ok");
    assert_eq!(v.panes.len(), 2);
    assert_eq!(v.panes[1].label(), "Other parameters");
    assert_eq!(
        json_of(code(&v.panes[1])),
        json!({"description": "Run the suite", "timeout": 120_000})
    );
}

#[test]
fn summarizes_the_call_by_its_payload() {
    assert_eq!(call_of("call-ok").summary, "bun test echo done");
}

#[test]
fn an_edit_becomes_a_diff() {
    let v = call_of("call-edit");
    let labels: Vec<&str> = v.panes.iter().map(ToolPane::label).collect();
    assert_eq!(labels, ["Edit", "Other parameters"]);
    let ToolPane::Diff { diff, language, .. } = &v.panes[0] else {
        panic!("expected a diff");
    };
    let kinds: Vec<RowKind> = diff.rows.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [RowKind::Ctx, RowKind::Del, RowKind::Add]);
    assert_eq!(*language, "typescript");
    assert_eq!(v.summary, "a.ts +1 −1");
    assert_eq!(
        json_of(code(&v.panes[1])),
        json!({"file_path": "/src/a.ts", "replace_all": false})
    );
}

#[test]
fn a_batch_of_edits_gets_a_numbered_diff_each() {
    let v = call_of("call-multi");
    let labels: Vec<&str> = v.panes.iter().map(ToolPane::label).collect();
    assert_eq!(labels, ["Edit 1", "Edit 2", "Other parameters"]);
    assert_eq!(v.summary, "b.md +2 −2");
}

#[test]
fn a_call_with_no_payload_arg_falls_back_to_the_whole_object() {
    let v = call_of("call-bad");
    assert_eq!(v.panes.len(), 1);
    assert_eq!(v.panes[0].label(), "Parameters");
    assert_eq!(json_of(code(&v.panes[0])), json!({"pattern": "TODO"}));
    assert!(v.summary.contains("TODO"), "{}", v.summary);
}

#[test]
fn plain_text_stays_verbatim() {
    let a = golden(false);
    let pane = result_pane(&a.events[event_at(&a, "tool-1", EventKind::ToolResult)]);
    let Body::Code { code, language } = pane else {
        panic!("expected code");
    };
    assert_eq!(language, "text");
    assert!(code.contains("export function login"));
}

/// Every string the tool panes would put on screen for a session.
fn rendered(a: &AnalyzedSession) -> String {
    let mut out = Vec::new();
    for e in &a.events {
        match e.kind {
            EventKind::ToolCall => {
                let v = call_view(e);
                out.push(v.summary.clone());
                for p in &v.panes {
                    match p {
                        ToolPane::Diff { old, new, diff, .. } => {
                            out.push(old.clone());
                            out.push(new.clone());
                            out.extend(diff.rows.iter().map(|r| r.text.clone()));
                        }
                        _ => out.push(p.text().unwrap_or_default().to_owned()),
                    }
                }
            }
            EventKind::ToolResult => {
                out.push(e.preview.clone());
                out.push(match result_pane(e) {
                    Body::Code { code, .. } => code,
                    Body::Prose { text } => text,
                });
            }
            _ => {}
        }
    }
    out.join("\n")
}

#[test]
fn nothing_rendered_from_a_redacted_session_holds_the_secret() {
    assert!(!rendered(&golden(true)).contains(SECRET));
}

#[test]
fn the_raw_session_still_holds_it() {
    assert!(rendered(&golden(false)).contains(SECRET));
}
