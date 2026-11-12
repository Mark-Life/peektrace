//! Parser facts from `opencode.test.ts` and the `OpenCode` cases of
//! `cost.test.ts`. The TS fixture `sessions-opencode/session-fixture.jsonl`
//! is reader output; here the same session is stored as DB rows. Part ids
//! get an ordinal prefix so SQL order (`message_id, id`) keeps file order.

mod common;

use common::{create_db, headers, opencode_roots, path_str, session_env, temp_dir};
use peektrace_core::opencode::{load_session, parse_opencode_session};
use peektrace_core::sessions::model::{EventKind, ParsedSession, Provider, TimelineEvent};
use peektrace_core::sessions::service::{Target, analyze_session};
use rusqlite::params;

const SESSION_ID: &str = "ses_test123";
const STARTED: &str = "2026-07-15T20:52:25.485Z";

const PARTS: [(&str, &str, &str); 13] = [
    (
        "prt_00_u1",
        "msg_user1",
        r#"{"type":"text","text":"hey there, list the files"}"#,
    ),
    (
        "prt_01_r1",
        "msg_asst1",
        r#"{"type":"reasoning","text":"The user wants a file listing."}"#,
    ),
    (
        "prt_02_t1",
        "msg_asst1",
        r#"{"type":"tool","tool":"glob","callID":"call-abc","state":{"status":"completed","input":{"pattern":"README*"},"output":"README.md\npackages/README.md","metadata":{"count":2},"title":"","time":{"start":1,"end":2}}}"#,
    ),
    (
        "prt_03_t2",
        "msg_asst1",
        r#"{"type":"tool","tool":"bash","callID":"call-err","state":{"status":"error","input":{"command":"exit 1"},"output":"command failed","time":{"start":3,"end":4}}}"#,
    ),
    (
        "prt_04_t3",
        "msg_asst1",
        r#"{"type":"tool","tool":"read","callID":"call-run","state":{"status":"running","input":{"path":"src/app.ts"}}}"#,
    ),
    (
        "prt_05_f1",
        "msg_asst1",
        r#"{"type":"file","filename":"src/app.ts","mime":"text/plain","url":"file:///Users/demo/proj/src/app.ts"}"#,
    ),
    (
        "prt_06_p1",
        "msg_asst1",
        r#"{"type":"patch","hash":"abc123","files":["src/app.ts"]}"#,
    ),
    (
        "prt_07_sn1",
        "msg_asst1",
        r#"{"type":"snapshot","snapshot":"snap-1"}"#,
    ),
    (
        "prt_08_st1",
        "msg_asst1",
        r#"{"type":"subtask","tool":"task"}"#,
    ),
    (
        "prt_09_ag1",
        "msg_asst1",
        r#"{"type":"agent","name":"reviewer"}"#,
    ),
    ("prt_10_rt1", "msg_asst1", r#"{"type":"retry"}"#),
    ("prt_11_s1", "msg_asst1", r#"{"type":"step-finish"}"#),
    ("prt_12_c1", "msg_asst1", r#"{"type":"compaction"}"#),
];

/// The fixture session in `<dir>/opencode.db`; returns the data dir.
fn fixture_dir() -> String {
    let dir = temp_dir("oc");
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, directory, title, version, model, agent, time_created, time_updated, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write) VALUES (?, '/Users/demo/proj', 'Test session', '1.18.2', '{\"id\":\"gpt-5.6-terra-fast\",\"providerID\":\"openai\"}', 'build', 1784148745485, 1784148748261, 0.01, 8305, 28, 0, 8192, 128)",
        [SESSION_ID],
    )
    .expect("session");
    let messages = [
        (
            "msg_user1",
            1_784_148_745_485_i64,
            r#"{"role":"user","time":{"created":1784148745485},"agent":"build"}"#,
        ),
        (
            "msg_asst1",
            1_784_148_745_500,
            r#"{"role":"assistant","modelID":"gpt-5.6-terra-fast","providerID":"openai","tokens":{"total":8311,"input":8305,"output":6,"reasoning":0,"cache":{"write":128,"read":8192}},"time":{"created":1784148745500}}"#,
        ),
    ];
    for (id, created, data) in messages {
        db.execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)",
            params![id, SESSION_ID, created, data],
        )
        .expect("message");
    }
    for (id, message, data) in PARTS {
        db.execute(
            "INSERT INTO part (id, message_id, session_id, data) VALUES (?, ?, ?, ?)",
            params![id, message, SESSION_ID, data],
        )
        .expect("part");
    }
    path_str(&dir)
}

fn parse() -> ParsedSession {
    let data = fixture_dir();
    let path = format!("{data}#{SESSION_ID}");
    parse_opencode_session(
        load_session(&path, "fallback-id", &data).as_ref(),
        &path,
        "fallback-id",
    )
}

fn find(p: &ParsedSession, f: impl Fn(&TimelineEvent) -> bool) -> Option<&TimelineEvent> {
    p.events.iter().find(|e| f(e))
}

fn result_for<'a>(p: &'a ParsedSession, call: &str) -> Option<&'a TimelineEvent> {
    find(p, |e| {
        e.kind == EventKind::ToolResult && e.tool_use_id.as_deref() == Some(call)
    })
}

#[test]
fn meta_comes_from_the_session_row() {
    let p = parse();
    assert_eq!(p.provider, Provider::Opencode);
    assert_eq!(p.session_id, SESSION_ID);
    assert_eq!(p.cwd.as_deref(), Some("/Users/demo/proj"));
    assert_eq!(p.title.as_deref(), Some("Test session"));
    assert_eq!(p.version.as_deref(), Some("1.18.2"));
    assert_eq!(p.subagents.len(), 0);
    assert_eq!(p.started_at.as_deref(), Some(STARTED));
    assert_eq!(p.ended_at.as_deref(), Some("2026-07-15T20:52:28.261Z"));
}

#[test]
fn models_dedup_session_and_message_ids() {
    assert_eq!(parse().models, ["gpt-5.6-terra-fast"]);
}

#[test]
fn native_context_window_is_unset() {
    assert_eq!(parse().native_context_window, None);
}

#[test]
fn step_parts_drop_from_the_timeline() {
    let kinds: Vec<_> = parse().events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "user-prompt",
            "assistant-thinking",
            "tool-call",
            "tool-result",
            "tool-call",
            "tool-result",
            // A running tool gives a call and no result.
            "tool-call",
            "attachment",
            "attachment",
            "attachment",
            "system",
            "system",
            "system",
            "compaction",
        ]
    );
}

#[test]
fn events_inherit_the_message_ts() {
    let p = parse();
    assert_eq!(p.events[0].ts.as_deref(), Some(STARTED));
    let thinking = find(&p, |e| e.kind == EventKind::AssistantThinking).expect("thinking");
    assert_eq!(thinking.ts.as_deref(), Some("2026-07-15T20:52:25.500Z"));
}

#[test]
fn running_tool_has_a_call_and_no_result() {
    let p = parse();
    let call = find(&p, |e| {
        e.kind == EventKind::ToolCall && e.tool_use_id.as_deref() == Some("call-run")
    })
    .expect("call");
    assert_eq!(call.tool_name.as_deref(), Some("read"));
    assert!(result_for(&p, "call-run").is_none());
}

#[test]
fn user_text_and_reasoning_map_to_prompt_and_thinking() {
    let p = parse();
    assert_eq!(p.events[0].kind, EventKind::UserPrompt);
    assert_eq!(p.events[0].body, "hey there, list the files");
    let thinking = find(&p, |e| e.kind == EventKind::AssistantThinking).expect("thinking");
    assert_eq!(thinking.body, "The user wants a file listing.");
}

#[test]
fn tool_part_pairs_call_and_result_by_call_id() {
    let p = parse();
    let call = find(&p, |e| {
        e.kind == EventKind::ToolCall && e.tool_name.as_deref() == Some("glob")
    })
    .expect("call");
    let result = result_for(&p, "call-abc").expect("result");
    assert_eq!(call.tool_use_id.as_deref(), Some("call-abc"));
    assert!(call.body.contains("README*"));
    assert!(result.body.contains("README.md"));
    assert_eq!(result.is_error, Some(false));
}

#[test]
fn error_tool_result_is_flagged() {
    let p = parse();
    let result = result_for(&p, "call-err").expect("result");
    assert_eq!(result.is_error, Some(true));
    assert_eq!(result.body, "command failed");
}

#[test]
fn compaction_is_indexed() {
    let p = parse();
    let compaction = find(&p, |e| e.kind == EventKind::Compaction).expect("compaction");
    assert_eq!(p.compaction_indexes, [compaction.index]);
}

#[test]
fn newer_part_types_map_to_kinds() {
    let p = parse();
    for t in ["file", "patch", "snapshot"] {
        let e = find(&p, |e| e.attachment_type.as_deref() == Some(t)).expect(t);
        assert_eq!(e.kind, EventKind::Attachment);
    }
    let titles: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.kind == EventKind::System)
        .map(|e| e.title.as_str())
        .collect();
    for t in ["subagent spawn", "agent", "retry"] {
        assert!(titles.contains(&t), "{t} in {titles:?}");
    }
}

#[test]
fn one_turn_per_assistant_message_with_its_usage() {
    let p = parse();
    let [t] = p.turns.as_slice() else {
        panic!("one turn, got {}", p.turns.len());
    };
    assert_eq!(t.request_id, "msg_asst1");
    assert_eq!(t.model, "gpt-5.6-terra-fast");
    assert_eq!(t.input_tokens, 8305.0);
    assert_eq!(t.cache_read_tokens, 8192.0);
    assert_eq!(t.cache_creation_tokens, 128.0);
    assert_eq!(t.output_tokens, 6.0);
    // Cache writes count toward context, not only input and cache reads.
    assert_eq!(t.context_tokens, 8305.0 + 8192.0 + 128.0);
}

#[test]
fn header_takes_the_project_from_the_directory() {
    let [h] = headers(&fixture_dir()).try_into().expect("one header");
    assert_eq!(h.agent.as_str(), "opencode");
    assert_eq!(h.project.as_str(), "proj");
    assert_eq!(h.cwd.as_deref(), Some("/Users/demo/proj"));
    assert_eq!(h.model.as_deref(), Some("gpt-5.6-terra-fast"));
    assert_eq!(h.title.as_deref(), Some("Test session"));
    assert_eq!(h.message_count, 2);
    assert!(h.size_bytes > 0);
    assert_eq!(h.started_at.as_deref(), Some(STARTED));
}

#[test]
fn unloadable_sessions_parse_empty() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, time_created, time_updated) VALUES ('broken', 1, 2)",
        [],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message (id, session_id, data) VALUES ('m', 'broken', '{\nnotjson')",
        [],
    )
    .expect("message");
    drop(db);
    let path = format!("{data}#broken");
    let broken = parse_opencode_session(load_session(&path, "x", &data).as_ref(), &path, "broken");
    let empty = parse_opencode_session(None, "/tmp/empty#x", "empty");
    assert_eq!(broken.provider, Provider::Opencode);
    assert_eq!(broken.session_id, "broken");
    for p in [&broken, &empty] {
        assert!(p.events.is_empty() && p.turns.is_empty() && p.compaction_indexes.is_empty());
    }
}

#[test]
fn session_cost_is_authoritative() {
    let data = fixture_dir();
    let roots = opencode_roots(&data);
    let a = analyze_session(session_env(&roots), Target::Lookup(SESSION_ID), None, false)
        .expect("analyze");
    let cost = a.cost.expect("cost");
    assert!(cost.from_agent);
    assert_eq!(cost.total_usd, Some(0.01));
}
