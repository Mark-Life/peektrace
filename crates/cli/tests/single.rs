//! End-to-end runs of `sessions analyze` and `sessions export`. Expected
//! output was produced by the TS CLI on the same files.

mod common;

use std::fs;

use common::Fixture;

#[test]
fn analyze_table() {
    let f = Fixture::new();
    let (code, out, err) = f.run(&["sessions", "analyze", "s1"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "field\tvalue\n\
         session\ts1\n\
         verdict\tHealthy\n\
         peak context\t6.0k / 1.0M (0.6%)\n\
         final context\t7\n\
         turns\t2\n\
         tool calls\t0\n\
         est. cost\t~$0.03\n  \
         subagents\t<$0.01\n\
         unpriced models\tmystery-model\n\
         dumb-zone cross turn\tnever\n\
         \n\
         budget (at peak)\ttokens\t%\n\
         (none)\n"
    );
}

#[test]
fn analyze_json_folds_subagents() {
    let f = Fixture::new();
    let (code, out, _) = f.run(&["sessions", "analyze", "s1", "--json"]);
    assert_eq!(code, 0);
    let y = format!(
        "{{\n      \"id\": \"agent-y\",\n      \"path\": \"{}\",\n      \"agentType\": \"Explore\",\n      \"description\": \"Find the file\",\n      \"turns\": 1,\n      \"peakContextTokens\": 400,\n      \"costUsd\": 0.002\n    }}",
        f.home
            .join(".claude/projects/-tmp-proj/s1/subagents/agent-y.jsonl")
            .display()
    );
    assert!(out.contains(&y), "{out}");
    assert!(out.contains(
        "\"cost\": {\n    \"source\": \"estimate\",\n    \"totalUsd\": 0.0254,\n    \"subagentsUsd\": 0.002,\n    \"unpricedModels\": [\n      \"mystery-model\"\n    ]\n  }"
    ));
}

#[test]
fn export_markdown() {
    let f = Fixture::new();
    let (code, out, err) = f.run(&["sessions", "export", "s1", "--format", "md"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let (head, rest) = out.split_once("- Exported: ").expect("export stamp");
    assert_eq!(
        head,
        "# First title\n\n\
         - Session: s1\n\
         - Provider: claude-code\n\
         - Models: claude-opus-5-5, mystery-model\n\
         - Branch: feat\n\
         - Directory: /tmp/proj\n\
         - Started: 2026-01-01T10:00:00.000Z\n\
         - Ended: 2026-01-01T09:00:00.000Z\n"
    );
    let (_, rest) = rest
        .split_once(" (redacted)\n\n## Subagents\n\n")
        .expect("subagents");
    let (subagents, timeline) = rest.split_once("\n\n## Timeline\n\n").expect("timeline");
    let mut subagents: Vec<&str> = subagents.lines().collect();
    subagents.sort_unstable();
    assert_eq!(
        subagents,
        [
            "- `agent-x`: 0 turns, peak 0 tok",
            "- `agent-y`: Explore, Find the file, 1 turns, peak 400 tok",
        ]
    );
    assert_eq!(
        timeline,
        "### 1. User (2026-01-01T10:00:00.000Z)\n\nhi\n\n\
         ### 2. harness (2026-01-01T10:00:01.000Z · meta)\n\nmeta\n\n\
         ### 3. tool-result (2026-01-01T10:00:06.000Z)\n\n*tool_result*\n\n```\nx\n```\n\n\
         ### 4. User (2026-01-01T09:00:00.000Z)\n\nlater line older ts\n"
    );
}

#[test]
fn export_subagent_json_with_parent() {
    let f = Fixture::new();
    let (code, out, _) = f.run(&[
        "sessions", "export", "agent-y", "--parent", "s1", "--format", "json",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("\"sessionId\": \"agent-y\""), "{out}");
    assert!(out.contains("\"body\": \"found it\""), "{out}");
}

#[test]
fn lookups_search_every_agent_without_a_note() {
    let f = Fixture::new();
    let opencode = f.home.join(".local/share/opencode");
    fs::create_dir_all(&opencode).expect("mkdir");
    fs::write(opencode.join("opencode.db"), "").expect("write");
    for args in [
        &["sessions", "analyze", "--latest"][..],
        &["sessions", "analyze", "s"],
        &["sessions", "analyze", "s1"],
        &["sessions", "export", "--cwd", "/tmp", "--format", "json"],
    ] {
        let (code, _, err) = f.run(args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
    }
    let (code, _, err) = f.run(&["sessions", "analyze", "zzz"]);
    assert_eq!(code, 1);
    assert_eq!(err, "Session \"zzz\" not found (searched all agents).\n");
}

/// TS fails on these rows; Rust skips the `null` line and reads non-array
/// assistant content as no events.
#[test]
fn null_rows_and_non_array_content_are_skipped() {
    let f = Fixture::new();
    let msg = |ts: u8, req: &str, content: &str| {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-01-01T10:00:0{ts}.000Z","requestId":"{req}","message":{{"model":"claude-opus-5-5","content":{content}}}}}"#
        )
    };
    let body = [
        r#"{"type":"user","timestamp":"2026-01-01T10:00:00.000Z","message":{"content":"x"}}"#
            .to_owned(),
        "null".to_owned(),
        msg(1, "r1", "7"),
        msg(2, "r2", "true"),
        msg(3, "r3", r#"{"type":"text","text":"obj"}"#),
        msg(4, "r4", r#"[{"type":"text","text":"y"}]"#),
    ]
    .join("\n");
    fs::write(f.home.join(".claude/projects/-tmp-proj/odd.jsonl"), body).expect("write");

    let (code, out, err) = f.run(&["sessions", "analyze", "odd", "--json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("\"sessionId\": \"odd\""), "{out}");

    let (code, out, err) = f.run(&["sessions", "export", "odd", "--format", "md"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let (_, timeline) = out.split_once("## Timeline\n\n").expect("timeline");
    assert_eq!(
        timeline,
        "### 1. User (2026-01-01T10:00:00.000Z)\n\nx\n\n\
         ### 2. Assistant (2026-01-01T10:00:04.000Z)\n\ny\n"
    );
}
