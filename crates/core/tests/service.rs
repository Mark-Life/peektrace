//! One session end to end through the service: lookup, subagent folding,
//! redaction and analysis. Ported from `service` and the Claude fold cases
//! of `swallowed-errors` TS tests.
#![allow(clippy::float_cmp)]

mod common;

use common::{TempDir, claude_roots, fixture, read_fixture, session_env};
use peektrace_core::AgentRoots;
use peektrace_core::sessions::model::{EventKind, ParsedSession};
use peektrace_core::sessions::resolve::SessionError;
use peektrace_core::sessions::service::{Target, analyze_session, get_session};
use serde_json::Value;

const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

fn golden_roots() -> AgentRoots {
    claude_roots(&fixture("sessions/home"), &fixture("sessions/projects"))
}

fn parse(redact: bool) -> ParsedSession {
    let roots = golden_roots();
    get_session(
        session_env(&roots),
        Target::Lookup(SESSION_ID),
        None,
        redact,
    )
    .expect("session")
}

#[test]
fn folds_the_subagent_transcript_into_the_parent() {
    let p = parse(true);
    let [sub] = p.subagents.as_slice() else {
        panic!("expected one subagent, got {}", p.subagents.len())
    };
    assert_eq!(sub.id, "agent-1");
    assert_eq!(sub.agent_type.as_deref(), Some("investigator"));
    assert_eq!(sub.turns, 2);
    assert_eq!(sub.peak_context_tokens, 700.0);
    assert_eq!(p.turns.len(), 3);
    let peak = p.turns.iter().fold(0.0_f64, |m, t| m.max(t.context_tokens));
    assert_eq!(peak, 1150.0);
}

fn tool_result_body(p: &ParsedSession) -> &str {
    &common::first_of(p, EventKind::ToolResult).body
}

#[test]
fn redaction_is_on_by_default() {
    let p = parse(true);
    assert!(tool_result_body(&p).contains("[REDACTED:anthropic-key]"));
    assert!(!tool_result_body(&p).contains("sk-ant-api03"));
}

#[test]
fn redact_false_leaves_the_transcript_verbatim() {
    assert!(tool_result_body(&parse(false)).contains("sk-ant-api03"));
}

#[test]
fn analyze_reproduces_golden_peak_and_budget() {
    let golden: Value =
        serde_json::from_str(&read_fixture("sessions/golden.json")).expect("golden");
    let roots = golden_roots();
    let a = analyze_session(session_env(&roots), Target::Lookup(SESSION_ID), None, true)
        .expect("analyze");
    assert_eq!(
        Some(a.peak_context_tokens),
        golden["peakContextTokens"].as_f64()
    );
    let budget: Vec<_> = a
        .budget
        .iter()
        .map(|b| (b.key.as_str().to_owned(), b.tokens))
        .collect();
    let want: Vec<_> = golden["budget"]
        .as_array()
        .expect("budget")
        .iter()
        .map(|b| {
            (
                b["key"].as_str().unwrap_or_default().to_owned(),
                b["tokens"].as_f64().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(budget, want);
}

#[test]
fn a_short_id_prefix_resolves_to_the_full_session() {
    let roots = golden_roots();
    let a = analyze_session(
        session_env(&roots),
        Target::Lookup(&SESSION_ID[..8]),
        None,
        true,
    )
    .expect("analyze");
    assert_eq!(a.parsed.session_id, SESSION_ID);
}

fn analyze_err(id: &str, parent: Option<&str>) -> SessionError {
    let roots = golden_roots();
    match analyze_session(session_env(&roots), Target::Lookup(id), parent, true) {
        Ok(a) => panic!("expected an error, got {}", a.parsed.session_id),
        Err(e) => e,
    }
}

#[test]
fn a_missing_session_is_not_found() {
    let e = analyze_err("does-not-exist", None);
    assert!(matches!(e, SessionError::NotFound { .. }), "{e:?}");
}

#[test]
fn a_subagent_id_alone_is_not_a_session() {
    analyze_err("agent-1", None);
}

#[test]
fn a_subagent_resolves_through_its_parent() {
    let roots = golden_roots();
    let a = analyze_session(
        session_env(&roots),
        Target::Lookup("agent-1"),
        Some(SESSION_ID),
        true,
    )
    .expect("analyze");
    assert_eq!(a.parsed.session_id, "agent-1");
    assert_eq!(a.turn_count, 2);
}

#[test]
fn short_ids_resolve_through_a_short_parent() {
    let roots = golden_roots();
    let a = analyze_session(
        session_env(&roots),
        Target::Lookup("agent-"),
        Some(&SESSION_ID[..8]),
        true,
    )
    .expect("analyze");
    assert_eq!(a.parsed.session_id, "agent-1");
}

#[test]
fn a_parent_without_that_subagent_is_not_found() {
    let e = analyze_err("agent-missing", Some(SESSION_ID));
    assert!(matches!(e, SessionError::NotFound { .. }), "{e:?}");
}

// swallowed-errors (Claude fold)

const GOOD: &str = "abcd1234-good";
const SUB: &str = r#"{"type":"assistant","isSidechain":true,"requestId":"q1","message":{"model":"claude-opus-4","usage":{"input_tokens":10,"output_tokens":1}}}
"#;

/// A session with one subagent, under a fresh projects root.
fn good_tree() -> TempDir {
    let dir = TempDir::new("swallowed");
    dir.write(
        &format!("projects/-p/{GOOD}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n",
    );
    dir.write(&format!("projects/-p/{GOOD}/subagents/agent-x.jsonl"), SUB);
    dir
}

fn parse_good(dir: &TempDir, id: &str) -> ParsedSession {
    let base = dir.str();
    let roots = claude_roots(&format!("{base}/home"), &format!("{base}/projects"));
    get_session(session_env(&roots), Target::Lookup(id), None, true).expect("session")
}

#[test]
fn a_subagent_resolve_failure_keeps_the_session() {
    let dir = good_tree();
    assert_eq!(parse_good(&dir, "abcd1234-goo").subagents.len(), 1);
    // A directory whose name ends in `.jsonl` is no transcript, but it makes
    // the subagent lookup's prefix match ambiguous, so folding fails.
    dir.mkdir("projects/-p/abcd1234-goofy.jsonl");
    let p = parse_good(&dir, "abcd1234-goo");
    assert_eq!(p.session_id, GOOD);
    assert_eq!(p.subagents.len(), 0);
}

#[cfg(unix)]
#[test]
fn an_unreadable_subagent_is_flagged() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = good_tree();
    let locked = dir.write(
        &format!("projects/-p/{GOOD}/subagents/agent-locked.jsonl"),
        SUB,
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    // Root reads it anyway: nothing to check then.
    if std::fs::read(&locked).is_ok() {
        return;
    }
    let p = parse_good(&dir, GOOD);
    let sub = p
        .subagents
        .iter()
        .find(|s| s.id == "agent-locked")
        .expect("locked subagent");
    assert_eq!(
        (sub.turns, sub.peak_context_tokens, sub.unreadable),
        (0, 0.0, true)
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod");
}
