//! `peektrace sessions analyze --latest / --cwd` over a throwaway home: two
//! Claude sessions (different cwds), a newer subagent transcript and one
//! newer Codex session. Ported from `apps/cli/test/sessions-analyze`.

mod common;

use common::{Home, claude_lines, claude_path, codex_lines, codex_path};
use serde_json::Value;

const OLD_ID: &str = "11111111-1111-4111-8111-111111111111";
const MID_ID: &str = "22222222-2222-4222-8222-222222222222";
const CODEX_ID: &str = "33333333-3333-4333-8333-333333333333";
const OLD_CWD: &str = "/home/demo/work";
const MID_CWD: &str = "/home/demo/work/sub";

fn home() -> Home {
    let h = Home::new("analyze");
    h.write(
        &claude_path("-home-demo-work", OLD_ID),
        &claude_lines("2026-03-01T10:00:00.000Z", 20_000, OLD_CWD),
    );
    h.write(
        &claude_path("-home-demo-work-sub", MID_ID),
        &claude_lines("2026-04-01T10:00:00.000Z", 40_000, MID_CWD),
    );
    h.write(
        &format!(".claude/projects/-home-demo-work-sub/{MID_ID}/subagents/agent-x.jsonl"),
        &claude_lines("2026-05-01T10:00:00.000Z", 99_000, MID_CWD),
    );
    h.write(
        &codex_path(CODEX_ID),
        &codex_lines(CODEX_ID, "/home/demo/other"),
    );
    h
}

fn analyze(h: &Home, args: &[&str]) -> common::Run {
    let mut all = vec!["sessions", "analyze"];
    all.extend_from_slice(args);
    h.run(&all)
}

fn json(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

fn session_of(out: &str) -> String {
    json(out)["sessionId"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// TS picks the Codex session; this build reads Claude only, so it picks the
/// newest Claude session and says Codex was not searched.
#[test]
fn latest_picks_the_newest_readable_session() {
    let r = analyze(&home(), &["--latest", "--json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(session_of(&r.out), MID_ID);
    assert!(
        r.err.contains("codex sessions are not searched"),
        "{}",
        r.err
    );
}

#[test]
fn cwd_ignores_a_newer_subagent_transcript() {
    let r = analyze(&home(), &["--cwd", MID_CWD, "--json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let a = json(&r.out);
    assert_eq!(a["sessionId"], MID_ID);
    assert_eq!(a["peakContextTokens"], 40_000);
}

#[test]
fn cwd_picks_the_newest_session_at_or_below_the_directory() {
    let r = analyze(&home(), &["--cwd", OLD_CWD, "--json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(session_of(&r.out), MID_ID);
}

#[test]
fn latest_and_cwd_combine_and_the_table_works() {
    let r = analyze(&home(), &["--latest", "--cwd", OLD_CWD]);
    assert_eq!(r.code, 0, "{}", r.err);
    for want in [MID_ID, "verdict", "est. cost"] {
        assert!(r.out.contains(want), "{want}: {}", r.out);
    }
}

#[test]
fn cwd_with_no_match_names_the_directory() {
    let r = analyze(&home(), &["--cwd", "/nowhere/at/all"]);
    assert_eq!(r.code, 1);
    assert!(
        r.err.contains("No sessions found under /nowhere/at/all"),
        "{}",
        r.err
    );
}

#[test]
fn latest_with_no_sessions_says_so() {
    let r = analyze(&Home::new("analyze-empty"), &["--latest"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("No sessions found."), "{}", r.err);
}

#[test]
fn no_id_and_no_flag_fails_with_usage() {
    let r = analyze(&home(), &[]);
    assert_eq!(r.code, 1);
    assert!(
        r.err
            .contains("Pass a session id, --latest, or --cwd <dir>"),
        "{}",
        r.err
    );
}

#[test]
fn an_id_with_latest_fails() {
    let r = analyze(&home(), &[OLD_ID, "--latest"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("not both"), "{}", r.err);
}

#[test]
fn an_explicit_id_works_with_json_before_or_after() {
    let h = home();
    assert_eq!(session_of(&analyze(&h, &["--json", OLD_ID]).out), OLD_ID);
    assert_eq!(session_of(&analyze(&h, &[OLD_ID, "--json"]).out), OLD_ID);
}

#[test]
fn parent_without_an_id_fails() {
    let r = analyze(&home(), &["--latest", "--parent", OLD_ID]);
    assert_eq!(r.code, 1);
    assert!(
        r.err.contains("--parent needs an explicit subagent id"),
        "{}",
        r.err
    );
}

#[test]
fn a_flag_missing_its_value_is_reported() {
    let h = home();
    for args in [&["--cwd"][..], &["--latest", "--cwd"]] {
        let r = analyze(&h, args);
        assert_eq!(r.code, 1, "{args:?}");
        assert!(r.err.contains("Missing value for --cwd"), "{}", r.err);
    }
}

#[test]
fn json_then_latest_parses_as_flags() {
    let r = analyze(&home(), &["--json", "--latest"]);
    assert_eq!(r.code, 0, "{}", r.err);
    json(&r.out);
}
