use super::*;
use crate::model::diff::RowKind;
use crate::model::fixtures::kind;

fn call(body: &str) -> TimelineEvent {
    TimelineEvent {
        body: body.into(),
        title: "tool_use".into(),
        ..kind(EventKind::ToolCall)
    }
}

fn result(body: &str) -> TimelineEvent {
    TimelineEvent {
        body: body.into(),
        ..kind(EventKind::ToolResult)
    }
}

fn code(p: &ToolPane) -> &str {
    match p {
        ToolPane::Code { code, .. } => code,
        _ => "",
    }
}

#[test]
fn payload_args_unwrap() {
    let v =
        call(r#"{"command":"bun test\necho done","description":"Run the suite","timeout":120000}"#);
    let view = call_view(&v);
    assert_eq!(
        view.panes[0],
        ToolPane::Code {
            label: "command".into(),
            code: "bun test\necho done".into(),
            language: "bash"
        }
    );
    assert_eq!(view.panes[1].label(), "Other parameters");
    assert_eq!(
        code(&view.panes[1]),
        "{\n  \"description\": \"Run the suite\",\n  \"timeout\": 120000\n}"
    );
    assert_eq!(view.summary, "bun test echo done");
}

#[test]
fn edits_become_diffs() {
    let v = call(
        r#"{"file_path":"/src/a.ts","old_string":"const a = 1;\nconst b = 2;","new_string":"const a = 1;\nconst b = 3;","replace_all":false}"#,
    );
    let view = call_view(&v);
    assert_eq!(
        view.panes.iter().map(ToolPane::label).collect::<Vec<_>>(),
        ["Edit", "Other parameters"]
    );
    let ToolPane::Diff { diff, language, .. } = &view.panes[0] else {
        panic!("expected a diff");
    };
    assert_eq!(
        diff.rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [RowKind::Ctx, RowKind::Del, RowKind::Add]
    );
    assert_eq!(*language, "typescript");
    assert_eq!(view.summary, "a.ts +1 −1");
    assert_eq!(
        code(&view.panes[1]),
        "{\n  \"file_path\": \"/src/a.ts\",\n  \"replace_all\": false\n}"
    );
    let multi = call(
        r##"{"file_path":"/docs/b.md","edits":[{"old_string":"# One","new_string":"# Uno"},{"old_string":"# Two","new_string":"# Dos"}]}"##,
    );
    let view = call_view(&multi);
    assert_eq!(
        view.panes.iter().map(ToolPane::label).collect::<Vec<_>>(),
        ["Edit 1", "Edit 2", "Other parameters"]
    );
    assert_eq!(view.summary, "b.md +2 −2");
}

#[test]
fn prose_and_code_args() {
    let view = call_view(&call(
        r##"{"description":"Survey","prompt":"# Task\n\n- read the code"}"##,
    ));
    assert_eq!(
        view.panes[0],
        ToolPane::Prose {
            label: "prompt".into(),
            text: "# Task\n\n- read the code".into()
        }
    );
    assert_eq!(view.summary, "# Task - read the code");
    let view = call_view(&call(r#"{"query":"a\nb","script":"x\ny"}"#));
    let kinds: Vec<_> = view
        .panes
        .iter()
        .map(|p| (p.label(), matches!(p, ToolPane::Code { .. })))
        .collect();
    assert_eq!(kinds, [("query", true), ("script", true)]);
    for key in ["file_path", "path", "notebook_path", "filename"] {
        let view = call_view(&call(&format!(
            r##"{{"content":"# x\ny","{key}":"a.py"}}"##
        )));
        assert!(
            matches!(
                view.panes[0],
                ToolPane::Code {
                    language: "text",
                    ..
                }
            ),
            "{key}"
        );
    }
}

#[test]
fn no_payload_falls_back_to_parameters() {
    let e = TimelineEvent {
        preview: "TODO".into(),
        ..call(r#"{"pattern":"TODO"}"#)
    };
    let view = call_view(&e);
    assert_eq!(view.panes.len(), 1);
    assert_eq!(view.panes[0].label(), "Parameters");
    assert_eq!(code(&view.panes[0]), r#"{"pattern":"TODO"}"#);
    assert_eq!(view.summary, "TODO");
    assert_eq!(code(&call_view(&call("")).panes[0]), EMPTY_BODY);
}

#[test]
fn results() {
    assert_eq!(
        result_pane(&result("plain output")),
        Body::Code {
            code: "plain output".into(),
            language: "text"
        }
    );
    let blocks = r###"[{"text":"## Report","type":"text"},{"text":"all good","type":"text"}]"###;
    assert_eq!(
        result_pane(&result(blocks)),
        Body::Prose {
            text: "## Report\n\nall good".into()
        }
    );
    let inner = r#"[{"type":"text","text":"{\"ok\":true}"}]"#;
    assert_eq!(
        result_pane(&result(inner)),
        Body::Code {
            code: "{\n  \"ok\": true\n}".into(),
            language: "json"
        }
    );
    assert_eq!(
        result_pane(&result(r#"{"b":1}"#)),
        Body::Code {
            code: "{\n  \"b\": 1\n}".into(),
            language: "json"
        }
    );
    let server = TimelineEvent {
        server_tool: Some(true),
        ..result("{\"a\":1}")
    };
    assert_eq!(
        result_pane(&server),
        Body::Prose {
            text: "{\"a\":1}".into()
        }
    );
    assert_eq!(
        result_pane(&result("")),
        Body::Code {
            code: EMPTY_BODY.into(),
            language: "text"
        }
    );
}

#[test]
fn pairing_and_state() {
    let with_id = |mut e: TimelineEvent, id: &str| {
        e.tool_use_id = Some(id.into());
        e
    };
    let events = vec![
        TimelineEvent {
            tool_name: Some("Bash".into()),
            ..with_id(call("{}"), "ok")
        },
        with_id(call("{}"), "bad"),
        with_id(call("{}"), "open"),
        with_id(result("ok"), "ok"),
        TimelineEvent {
            is_error: Some(true),
            ..with_id(result("boom"), "bad")
        },
    ];
    let ix = index_tools(&events);
    assert_eq!(tool_state(&events, &events[0], &ix), ToolState::Completed);
    assert_eq!(tool_state(&events, &events[1], &ix), ToolState::Error);
    assert_eq!(tool_state(&events, &events[2], &ix), ToolState::Unanswered);
    assert_eq!(tool_state(&events, &events[4], &ix), ToolState::Error);
    assert_eq!(tool_name_of(&events, &events[3], &ix), "Bash");
    assert_eq!(tool_name_of(&events, &events[4], &ix), "tool");
}

#[test]
fn small_helpers() {
    assert_eq!(one_line("  a \n\t b  "), "a b");
    assert_eq!(one_line(&"x".repeat(201)), format!("{}…", "x".repeat(200)));
    assert_eq!(language_of_path("/a/b.tsx"), "typescript");
    assert_eq!(language_of_path("x.jsonc"), "json");
    assert_eq!(language_of_path("README.md"), "markdown");
    assert_eq!(language_of_path("a.py"), "text");
    let file = |t: &str| TimelineEvent {
        title: t.into(),
        ..kind(EventKind::Attachment)
    };
    assert_eq!(body_view(&file("file: a.ts")), Some("typescript"));
    assert_eq!(body_view(&file("file: a.md")), None);
    assert_eq!(body_view(&file("hook")), None);
}

// Ports of `tool-event.test.ts` result and body cases the tests above do not
// hold with the same input.

#[test]
fn a_text_block_holding_json_stays_a_json_code_block() {
    let blocks = r#"[{"text":"{\"items\":[1,2]}","type":"text"}]"#;
    assert_eq!(
        result_pane(&result(blocks)),
        Body::Code {
            code: "{\n  \"items\": [\n    1,\n    2\n  ]\n}".into(),
            language: "json"
        }
    );
}

#[test]
fn a_server_tool_result_is_prose_a_request_id_alone_is_not() {
    let hits = TimelineEvent {
        request_id: Some("r1".into()),
        server_tool: Some(true),
        title: "web_search result".into(),
        ..result("- [Bun 1.4](https://bun.sh/blog/bun-v1.4)")
    };
    assert_eq!(
        result_pane(&hits),
        Body::Prose {
            text: hits.body.clone()
        }
    );
    let shell = TimelineEvent {
        server_tool: None,
        body: "a.md\nb.md".into(),
        ..hits
    };
    assert!(matches!(result_pane(&shell), Body::Code { .. }));
}

#[test]
fn body_views_by_title() {
    let file = |t: &str| TimelineEvent {
        title: t.into(),
        body: "x".into(),
        ..kind(EventKind::Attachment)
    };
    assert_eq!(body_view(&file("skills_instructions")), None);
    assert_eq!(body_view(&file("file: CLAUDE.md")), None);
    assert_eq!(body_view(&file("file: index.ts")), Some("typescript"));
    assert_eq!(
        body_view(&file("opened_file_in_ide: main.py")),
        Some("text")
    );
}
