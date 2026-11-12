//! Codex and Pi parsing, listing and lookup against the TS test fixtures in
//! `packages/core/test/fixtures`. Expected values come from the TS tests and
//! the TS CLI on the same files.

use std::fs;
use std::path::{Path, PathBuf};

use peektrace_core::codex::parse_codex_session;
use peektrace_core::js::time::Utc;
use peektrace_core::pi::parse_pi_session;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_core::sessions::model::{ContextEdit, EventKind, LoadedCategory, ParsedSession};
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::{Agent, AgentRoots, ListFilter, SessionHeader, list_sessions};

fn fixture(rel: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/core/test/fixtures")
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

fn read(rel: &str) -> String {
    fs::read_to_string(fixture(rel)).expect("fixture")
}

fn codex(name: &str) -> ParsedSession {
    let path = fixture(&format!("sessions-codex/{name}.jsonl"));
    parse_codex_session(
        &read(&format!("sessions-codex/{name}.jsonl")),
        &path,
        "fallback",
    )
}

fn pi_text(text: &str) -> ParsedSession {
    parse_pi_session(text, "/tmp/pi.jsonl", "fallback")
}

fn pi(name: &str) -> ParsedSession {
    pi_text(&read(&format!("sessions-pi/{name}.jsonl")))
}

const CODEX_ID: &str = "019f42f4-342d-7b30-9551-a78e5a2cbe5e";

#[test]
fn codex_meta_and_window() {
    let p = codex("rollout-fixture");
    assert_eq!(p.provider.as_str(), "codex");
    assert_eq!(p.session_id, CODEX_ID);
    assert_eq!(
        p.cwd.as_deref(),
        Some("/Users/andrey-m/Code/personal/peephole")
    );
    assert_eq!(p.git_branch.as_deref(), Some("feature/parsers"));
    assert_eq!(p.version.as_deref(), Some("0.134.0"));
    assert_eq!(p.models, ["gpt-5.5"]);
    assert_eq!(p.title, None);
    assert_eq!(p.started_at.as_deref(), Some("2026-07-08T10:00:00.000Z"));
    assert_eq!(p.ended_at.as_deref(), Some("2026-07-08T10:00:12.000Z"));
    assert_eq!(p.native_context_window, Some(258_400.0));
}

#[test]
fn codex_turns_use_the_per_call_delta() {
    let p = codex("rollout-fixture");
    let turns: Vec<_> = p
        .turns
        .iter()
        .map(|t| {
            (
                t.request_id.as_str(),
                t.context_tokens,
                t.cache_read_tokens,
                t.input_tokens,
                t.output_tokens,
                t.event_indexes.clone(),
            )
        })
        .collect();
    assert_eq!(
        turns,
        [
            (
                "turn-aaa#1",
                13_917.0,
                11_648.0,
                2_269.0,
                201.0,
                vec![2, 3, 4]
            ),
            ("turn-aaa#2", 15_200.0, 13_000.0, 2_200.0, 300.0, vec![7]),
        ]
    );
    assert_eq!(p.turns[0].ts.as_deref(), Some("2026-07-08T10:00:06.000Z"));
    assert_eq!(p.turns[0].cache_creation_tokens, 0.0);
}

#[test]
fn codex_events() {
    let p = codex("rollout-fixture");
    let kinds: Vec<_> = p.events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "user-prompt",
            "attachment",
            "assistant-thinking",
            "assistant-text",
            "tool-call",
            "tool-result",
            "attachment",
            "assistant-text",
            "tool-call",
            "tool-result",
        ]
    );
    let env = &p.events[1];
    assert_eq!(env.attachment_type.as_deref(), Some("environment_context"));
    assert_eq!(env.loaded_category, Some(LoadedCategory::Other));
    assert_eq!(
        p.events[6].attachment_type.as_deref(),
        Some("collaboration_mode")
    );
    let thinking = &p.events[2];
    assert_eq!((thinking.body.as_str(), thinking.tokens_est), ("", 0.0));
    assert_eq!(
        thinking.preview,
        "(reasoning encrypted \u{2014} not stored)"
    );
    let result = |id: &str| {
        p.events
            .iter()
            .find(|e| e.kind == EventKind::ToolResult && e.tool_use_id.as_deref() == Some(id))
            .expect("result")
    };
    assert_eq!(result("call_1").is_error, Some(true));
    assert!(result("call_2").body.contains("Success"));
    assert_eq!(result("call_2").is_error, Some(false));
    assert_eq!(p.events[3].request_id.as_deref(), Some("turn-aaa#1"));
    assert_eq!(p.events[5].request_id, None);
}

#[test]
fn codex_garbage_never_fails() {
    let text = "not json at all\n{\"type\":\"response_item\",\"payload\":{\n\n   \nnull\n5";
    let p = parse_codex_session(text, "/tmp/broken.jsonl", "broken");
    assert_eq!(p.session_id, "broken");
    assert!(p.events.is_empty() && p.turns.is_empty() && p.models.is_empty());
}

#[test]
fn codex_budget_skips_re_emitted_totals_and_aborted_calls() {
    let p = codex("rollout-budget");
    let ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(
        ids,
        ["turn-a#1", "turn-a#2", "turn-a#3", "turn-c#1", "turn-c#2"]
    );
    assert_eq!(p.events[14].kind, EventKind::AssistantThinking);
    assert_eq!(p.events[14].request_id, None);
    assert!(
        p.events
            .iter()
            .filter(|e| !e.kind.is_output())
            .all(|e| e.request_id.is_none())
    );
}

#[test]
fn codex_fork_skips_the_parents_replayed_count() {
    let p = codex("rollout-fork");
    let ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(ids, ["child-turn#1", "child-turn#2"]);
    assert_eq!(p.turns[0].event_indexes, [4, 5]);
}

#[test]
fn pi_meta_turns_and_window() {
    let p = pi("pi-fixture");
    assert_eq!(p.provider.as_str(), "pi");
    assert_eq!(p.session_id, "019f3788-3bb1-7302-a2bf-55b30354d752");
    assert_eq!(p.cwd.as_deref(), Some("/Users/demo/proj"));
    assert_eq!(p.version.as_deref(), Some("3"));
    assert_eq!(p.git_branch, None);
    assert_eq!(p.models, ["qwen/qwen3.6-27b", "claude-opus-4-8"]);
    let turn = |id: &str| p.turns.iter().find(|t| t.request_id == id);
    let a = turn("aaaa1111").expect("turn");
    assert_eq!(
        (
            a.input_tokens,
            a.cache_read_tokens,
            a.cache_creation_tokens,
            a.output_tokens,
            a.context_tokens
        ),
        (11.0, 5_750.0, 0.0, 35.0, 5_761.0)
    );
    assert_eq!(a.ts.as_deref(), Some("2026-07-06T13:05:20.000Z"));
    assert_eq!(turn("bbbb2222").map(|t| t.context_tokens), Some(120.0));
    assert!(turn("cccc3333").is_none());
    assert_eq!(p.native_context_window, Some(200_000.0));
    let empty = p
        .events
        .iter()
        .find(|e| e.kind == EventKind::AssistantThinking && e.body.is_empty())
        .expect("empty thinking");
    assert_eq!(empty.preview, "(content not stored in transcript)");
}

#[test]
fn pi_window_is_dropped_by_a_larger_call() {
    let text = read("sessions-pi/pi-fixture.jsonl");
    let at = text.find("\"input\":").expect("usage");
    let end = at + text[at..].find(',').expect("comma");
    let big = format!("{}\"input\":300000{}", &text[..at], &text[end..]);
    assert_eq!(pi_text(&big).native_context_window, None);
    let one = r#"{"type":"session","version":1,"id":"s1","cwd":"/tmp"}"#;
    assert_eq!(pi_text(one).native_context_window, None);
}

const BRANCHED: &str = r#"{"type":"session","id":"s","cwd":"/tmp"}
{"type":"message","id":"u1","parentId":null,"message":{"role":"user","content":"start"}}
{"type":"message","id":"a1","parentId":"u1","message":{"role":"assistant","model":"m","content":[{"type":"text","text":"dead"}],"usage":{"input":5}}}
{"type":"session_info","id":"i1","parentId":"a1","name":"Title"}
{"type":"context_edit","id":"x1","parentId":"a1","targetId":"u1","replacement":null}
{"type":"message","id":"a2","parentId":"u1","message":{"role":"assistant","model":"m","content":[{"type":"text","text":"live"}],"usage":{"input":7}}}"#;

#[test]
fn pi_active_branch() {
    let p = pi_text(BRANCHED);
    let bodies: Vec<_> = p.events.iter().map(|e| e.body.as_str()).collect();
    assert_eq!(bodies, ["start", "live"]);
    assert!(p.events.iter().all(|e| e.context_edit.is_none()));
    let ids: Vec<_> = p.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(ids, ["a1", "a2"]);
    assert_eq!(p.turns[0].event_indexes, Vec::<i64>::new());
    assert_eq!(p.title.as_deref(), Some("Title"));
    let untrusted = BRANCHED.replace(",\"parentId\":\"u1\"", "");
    let bodies: Vec<_> = pi_text(&untrusted)
        .events
        .into_iter()
        .map(|e| e.body)
        .collect();
    assert_eq!(bodies, ["start", "dead", "live"]);
}

#[test]
fn pi_context_edits() {
    let p = pi("pi-context-edit");
    let at = |i: usize| p.events.iter().find(|e| e.index == i).expect("event");
    assert_eq!(at(8).body, "A".repeat(400));
    assert_eq!(
        at(8).context_edit,
        Some(ContextEdit::Removed {
            at_index: 9.0,
            steps: None
        })
    );
    assert_eq!(at(4).body, "[big.txt elided]");
    assert_eq!(
        at(4).context_edit,
        Some(ContextEdit::Replaced {
            at_index: 10.0,
            original_tokens_est: 1000.0,
            original: Some("X".repeat(4000)),
            steps: None,
        })
    );
    let steps = at(5).context_edit.as_ref().map(|e| e.steps().to_vec());
    assert_eq!(
        steps.map(|s| (s.len(), s[0].at_index, s[0].tokens_est)),
        Some((1, 11.0, 3.0))
    );
    assert_eq!(at(6).kind, EventKind::AssistantText);
    assert_eq!(at(6).body, "Short summary.");
    assert_eq!(
        p.events.iter().filter(|e| e.context_edit.is_some()).count(),
        4
    );
    let linked: Vec<Vec<&str>> = p
        .turns
        .iter()
        .map(|t| {
            t.event_indexes
                .iter()
                .map(|&i| p.events[usize::try_from(i).expect("index")].body.as_str())
                .collect()
        })
        .collect();
    let call = "{\n  \"path\": \"big.txt\"\n}";
    let long = "A".repeat(400);
    assert_eq!(
        linked,
        [
            vec!["", call],
            vec!["Short summary."],
            vec![long.as_str()],
            vec!["Done."]
        ]
    );
    let steps = pi("pi-context-edit-steps");
    let ev = steps.events.iter().find(|e| e.index == 4).expect("event");
    assert_eq!(ev.tokens_measured, Some(1180.0));
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("peektrace-agents-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// Claude golden root, the Codex fixtures and the Pi fixtures under one
/// slug, as the parity `agents-fixtures` suite sets them.
fn agent_roots(pi_root: &Path) -> AgentRoots {
    let slug = pi_root.join("--tmp-pi--");
    fs::create_dir_all(&slug).expect("mkdir");
    for entry in fs::read_dir(fixture("sessions-pi")).expect("dir").flatten() {
        fs::copy(entry.path(), slug.join(entry.file_name())).expect("copy");
    }
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(fixture("sessions/projects")),
        codex_sessions: Some(fixture("sessions-codex")),
        pi_sessions: Some(pi_root.to_string_lossy().into_owned()),
        ..RootsEnv::default()
    };
    AgentRoots::resolve(&env, &Settings::default())
}

fn by_agent(headers: &[SessionHeader], agent: Agent) -> Vec<&SessionHeader> {
    headers.iter().filter(|h| h.agent == agent).collect()
}

#[test]
fn lists_and_resolves_across_agents() {
    let dir = temp_dir("list");
    let roots = agent_roots(&dir);
    let all = list_sessions(&roots, &ListFilter::default());
    assert_eq!(all.skipped, [Agent::Opencode]);
    assert_eq!(by_agent(&all.headers, Agent::Claude).len(), 1);
    let codex = by_agent(&all.headers, Agent::Codex);
    assert_eq!(codex.len(), 4);
    let fixture_row = codex
        .iter()
        .find(|h| h.id.as_str() == "rollout-fixture")
        .expect("row");
    assert_eq!(fixture_row.project.as_str(), "peephole");
    assert_eq!(fixture_row.message_count, 3);
    assert_eq!(fixture_row.cost_usd, None);
    let pis = by_agent(&all.headers, Agent::Pi);
    assert_eq!(pis.len(), 4);
    let pi_row = pis
        .iter()
        .find(|h| h.id.as_str() == "pi-fixture")
        .expect("row");
    assert_eq!(
        (
            pi_row.project.as_str(),
            pi_row.message_count,
            pi_row.model.as_deref()
        ),
        ("--tmp-pi--", 5, Some("qwen/qwen3.6-27b"))
    );
    let only = |project: &str| {
        list_sessions(
            &roots,
            &ListFilter {
                agent: None,
                project: Some(project.to_owned()),
            },
        )
        .headers
        .len()
    };
    assert_eq!(
        (only("peephole"), only("--tmp-pi--"), only("nope")),
        (1, 4, 0)
    );

    let env = SessionEnv {
        roots: &roots,
        unsearched: &[],
        local: &Utc,
    };
    let a = analyze_session(env, Target::Lookup("rollout-fixture"), None, true).expect("codex");
    assert_eq!(a.parsed.session_id, CODEX_ID);
    assert_eq!(a.context_window, 258_400.0);
    for h in &pis {
        let a = analyze_session(env, Target::Lookup(h.id.as_str()), None, false).expect("pi");
        let listed = analyze_session(env, Target::Listed(h), None, false).expect("listed");
        assert_eq!(listed.parsed, a.parsed, "{}", h.id);
        assert_eq!(a.cost.and_then(|c| c.total_usd), h.cost_usd, "{}", h.id);
    }
    let err = |id: &str, parent: Option<&str>| {
        analyze_session(env, Target::Lookup(id), parent, true)
            .err()
            .map(|e| e.to_string())
    };
    assert_eq!(
        err("rollout", None).as_deref(),
        Some("Session \"rollout\" not found (searched all agents).")
    );
    assert_eq!(
        err("x", Some("pi-fixture")).as_deref(),
        Some("Session \"x\" not found (searched subagents of pi-fixture).")
    );
    let path = fixture("sessions-codex/rollout-fork.jsonl");
    let direct = analyze_session(env, Target::Lookup(&path), None, true).expect("path");
    assert_eq!(direct.parsed.provider.as_str(), "codex");
    let _ = fs::remove_dir_all(&dir);
}
