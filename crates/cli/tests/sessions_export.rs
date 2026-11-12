//! `peektrace sessions export` over a throwaway home: one Claude session
//! whose text carries fake secrets and a home path, and one newer Codex
//! session. Ported from `apps/cli/test/sessions-export`.

mod common;

use common::{Home, claude_path, codex_lines, codex_path};
use serde_json::{Value, json};

const CLAUDE_ID: &str = "11111111-1111-4111-8111-111111111111";
const CODEX_ID: &str = "33333333-3333-4333-8333-333333333333";
const CWD: &str = "/home/demo/work";

fn anthropic_key() -> String {
    format!("sk-ant-{}", "a1B2c3D4e5".repeat(5))
}

fn github_token() -> String {
    format!("ghp_{}abcdef", "x9Y8z7W6v5".repeat(3))
}

fn home() -> Home {
    let h = Home::new("export");
    let at = "2026-03-01T10:00:00.000Z";
    let text = format!(
        "use key {} and {} in {CWD}/a.ts <b>x</b>",
        anthropic_key(),
        github_token()
    );
    h.write(
        &claude_path("-home-demo-work", CLAUDE_ID),
        &[
            json!({"type":"user","cwd":CWD,"timestamp":at,"message":{"content":text}}),
            json!({"type":"assistant","cwd":CWD,"timestamp":at,"requestId":"req-1",
                "message":{"model":"claude-opus-4-1","content":[{"type":"text","text":"done"}],
                    "usage":{"input_tokens":1000,"cache_read_input_tokens":0,
                        "cache_creation_input_tokens":0,"output_tokens":10}}}),
        ],
    );
    h.write(
        &codex_path(CODEX_ID),
        &codex_lines(CODEX_ID, "/home/demo/other"),
    );
    h
}

fn export(h: &Home, args: &[&str]) -> common::Run {
    let mut all = vec!["sessions", "export"];
    all.extend_from_slice(args);
    h.run(&all)
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[test]
fn each_format_prints_to_stdout() {
    let h = home();
    let html = export(&h, &[CLAUDE_ID, "--format", "html"]);
    assert_eq!(html.code, 0, "{}", html.err);
    assert!(html.out.to_lowercase().contains("<!doctype html"));
    assert!(html.out.contains("&lt;b&gt;x&lt;/b&gt;"));
    let md = export(&h, &[CLAUDE_ID, "--format", "md"]);
    assert_eq!(md.code, 0, "{}", md.err);
    assert!(md.out.starts_with("# "));
    let js = export(&h, &[CLAUDE_ID, "--format", "json"]);
    assert_eq!(js.code, 0, "{}", js.err);
    assert_eq!(json_of(&js.out)["schema"], "peektrace.session-export");
}

#[test]
fn default_is_redacted_markdown() {
    let h = home();
    let r = export(&h, &[CLAUDE_ID]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.starts_with("# "));
    assert!(r.out.contains("[REDACTED"));
    assert!(!r.out.contains(&anthropic_key()));
    assert!(!r.out.contains(&github_token()));
    assert!(!r.out.contains("/home/demo"));
    assert!(!r.out.contains(&h.str()));
}

#[test]
fn no_redact_keeps_the_text() {
    let h = home();
    let r = export(&h, &[CLAUDE_ID, "--no-redact"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains(&anthropic_key()));
    assert!(r.out.contains(&github_token()));
    assert!(!r.out.contains(&h.str()));
}

#[test]
fn json_alone_gives_json_before_or_after_the_subcommand() {
    let h = home();
    assert_eq!(
        json_of(&export(&h, &[CLAUDE_ID, "--json"]).out)["version"],
        1
    );
    let r = h.run(&["--json", "sessions", "export", CLAUDE_ID]);
    assert_eq!(json_of(&r.out)["version"], 1);
}

#[test]
fn out_writes_the_file_with_the_format_from_its_extension() {
    let h = home();
    let target = h.dir.join("out.html");
    let target_s = target.to_string_lossy().into_owned();
    let r = export(&h, &[CLAUDE_ID, "--out", &target_s]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "");
    assert!(r.err.contains(&format!("Wrote {target_s}")), "{}", r.err);
    let body = std::fs::read_to_string(&target).expect("written");
    assert!(body.to_lowercase().contains("<!doctype html"));
}

#[test]
fn format_wins_over_the_out_extension() {
    let h = home();
    let target = h.dir.join("x.html");
    export(
        &h,
        &[
            CLAUDE_ID,
            "--format",
            "json",
            "--out",
            &target.to_string_lossy(),
        ],
    );
    let body = std::fs::read_to_string(&target).expect("written");
    assert_eq!(json_of(&body)["version"], 1);
}

#[test]
fn an_unknown_id_fails_with_a_message() {
    let r = export(&home(), &["99999999-9999-4999-8999-999999999999"]);
    assert_ne!(r.code, 0);
    assert_ne!(r.err.len(), 0);
}

#[test]
fn a_bad_format_is_rejected() {
    let r = export(&home(), &[CLAUDE_ID, "--format", "pdf"]);
    assert_ne!(r.code, 0);
    assert!(!r.out.contains("# "));
}

#[test]
fn no_id_and_no_flag_fails_with_usage() {
    let r = export(&home(), &[]);
    assert_eq!(r.code, 1);
    assert!(
        r.err
            .contains("Pass a session id, --latest, or --cwd <dir>"),
        "{}",
        r.err
    );
}

#[test]
fn latest_exports_a_non_claude_session() {
    let h = home();
    let r = export(&h, &["--latest", "--format", "json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let doc = json_of(&r.out);
    assert_eq!(doc["session"]["sessionId"], CODEX_ID);
    assert_eq!(doc["session"]["provider"], "codex");
    assert!(!r.out.contains(&h.str()));
}

#[test]
fn out_dash_prints_to_stdout_and_writes_no_file() {
    let r = export(&home(), &[CLAUDE_ID, "--out", "-"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.starts_with("# "));
    assert!(!r.err.contains("Wrote"));
}

#[test]
fn out_into_a_missing_directory_fails_with_one_line() {
    let h = home();
    let target = h
        .dir
        .join("no-such-dir/x.md")
        .to_string_lossy()
        .into_owned();
    let r = export(&h, &[CLAUDE_ID, "--out", &target]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("Cannot write"), "{}", r.err);
    assert!(r.err.contains("the directory does not exist"), "{}", r.err);
    assert!(!r.err.contains("ENOENT"), "{}", r.err);
    assert_eq!(r.err.matches(&target).count(), 1, "{}", r.err);
}
