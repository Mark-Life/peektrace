//! `peektrace sessions search`, run as a user runs it: a real process over a
//! throwaway home holding synthetic sessions (port of
//! `apps/cli/test/sessions-search.test.ts`).

mod common;

use common::{Home, Run};
use serde_json::{Value, json};

const NEW_ID: &str = "11111111-1111-4111-8111-111111111111";
const OLD_ID: &str = "22222222-2222-4222-8222-222222222222";
const DASH_ID: &str = "33333333-3333-4333-8333-333333333333";
const FAKE_KEY: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

fn user(at: &str, text: &str) -> Value {
    json!({"type":"user","cwd":"/home/demo/work","timestamp":at,"message":{"content":text}})
}

fn home() -> Home {
    let h = Home::new("search");
    let dir = ".claude/projects/-home-demo-work";
    h.write(
        &format!("{dir}/{NEW_ID}.jsonl"),
        &[
            user(
                "2026-05-01T10:00:00.000Z",
                &format!("use {FAKE_KEY} for the walrus job"),
            ),
            user("2026-05-01T10:00:01.000Z", "walrus again"),
        ],
    );
    h.write(
        &format!("{dir}/{OLD_ID}.jsonl"),
        &[user("2026-02-01T10:00:00.000Z", "an old walrus note")],
    );
    h.write(
        &format!(".claude/projects/-home-demo-dash/{DASH_ID}.jsonl"),
        &[user("2026-01-01T10:00:00.000Z", "run with -v flag zebra")],
    );
    h
}

fn search(h: &Home, args: &[&str]) -> Run {
    let mut all = vec!["sessions", "search"];
    all.extend_from_slice(args);
    h.run(&all)
}

fn json_of(r: &Run) -> Value {
    serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}", r.out))
}

#[test]
fn prints_one_row_per_hit_newest_session_first_with_line_numbers() {
    let h = home();
    let r = search(&h, &["walrus"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let lines: Vec<&str> = r.out.trim().split('\n').collect();
    assert_eq!(
        lines[0].split('\t').collect::<Vec<_>>(),
        [
            "session", "agent", "project", "updated", "matches", "snippet"
        ]
    );
    let first: Vec<&str> = lines[1].split('\t').collect();
    assert_eq!(first[..2], ["11111111", "claude"]);
    assert_eq!(first[4], "2");
    assert!(first[5].starts_with("L1  "), "{}", first[5]);
    assert!(
        lines[2]
            .split('\t')
            .nth(5)
            .unwrap_or_default()
            .starts_with("L2  ")
    );
    assert_eq!(lines[3].split('\t').next(), Some("22222222"));
}

#[test]
fn json_prints_the_raw_result() {
    let h = home();
    let r = search(&h, &["walrus", "--json", "--limit", "1"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let v = json_of(&r);
    let ids: Vec<&str> = v["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert_eq!(ids, [NEW_ID]);
    assert_eq!(v["truncated"], json!(true));
    assert!(v["scannedSessions"].is_number());
}

#[test]
fn redacts_secrets_by_default_and_keeps_them_with_no_redact() {
    let h = home();
    assert!(!search(&h, &["walrus job"]).out.contains(FAKE_KEY));
    assert!(
        search(&h, &["walrus job", "--no-redact"])
            .out
            .contains(FAKE_KEY)
    );
}

#[test]
fn since_narrows_by_update_time() {
    let h = home();
    let r = search(&h, &["walrus", "--since", "2026-04-01"]);
    assert!(r.out.contains("11111111"), "{}", r.out);
    assert!(!r.out.contains("22222222"), "{}", r.out);
}

#[test]
fn no_match_exits_0_and_says_so_on_stderr() {
    let h = home();
    let r = search(&h, &["no-such-phrase"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.out, "");
    assert!(r.err.contains("no matches"), "{}", r.err);
}

#[test]
fn bad_input_is_a_user_error() {
    let h = home();
    let cases: [&[&str]; 4] = [
        &["walrus", "--agent", "nope"],
        &["walrus", "--since", "soon"],
        &["walrus", "--limit", "-1"],
        &["   "],
    ];
    for args in cases {
        let r = search(&h, args);
        assert_ne!(r.code, 0, "{args:?}");
        assert_ne!(r.err, "", "{args:?}");
    }
}

#[test]
fn help_exits_0() {
    let h = home();
    let r = search(&h, &["--help"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("--no-redact"), "{}", r.out);
}

#[test]
fn text_after_double_dash_is_the_query_even_when_it_looks_like_a_flag() {
    let h = home();
    let dash = search(&h, &["--", "-v"]);
    assert_eq!(dash.code, 0, "{}", dash.err);
    assert!(dash.out.contains("33333333"), "{}", dash.out);
    assert!(search(&h, &["--", "with -v flag"]).out.contains("33333333"));
    let version = search(&h, &["--", "--version zebra"]);
    assert!(
        !version.out.contains(env!("CARGO_PKG_VERSION")),
        "{}",
        version.out
    );
}

#[test]
fn a_project_shown_in_the_results_works_as_project() {
    let h = home();
    let all = json_of(&search(&h, &["walrus", "--json"]));
    let shown = all["sessions"][0]["project"]
        .as_str()
        .expect("project")
        .to_owned();
    assert!(!shown.contains("home"), "{shown}");
    let r = search(&h, &["walrus", "--project", &shown, "--json"]);
    assert_eq!(
        json_of(&r)["sessions"].as_array().map(Vec::len),
        Some(2),
        "{}",
        r.out
    );
}

#[test]
fn several_unquoted_words_form_one_phrase() {
    let h = home();
    let r = search(&h, &["an", "old", "walrus"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("an old walrus note"), "{}", r.out);
}

/// TS checks the telemetry db; Rust has no telemetry yet, so the same
/// guarantee is checked on the `--otel` span echo: typed words never reach a
/// span, even when validation fails.
#[test]
fn typed_words_never_reach_telemetry_even_in_validation_errors() {
    let h = home();
    let attempts: [&[&str]; 3] = [
        &["zebrasecret", "--limit", "abcsecret"],
        &["zebrasecret", "--agent", "quokkasecret"],
        &["zebrasecret", "--since", "quokkasecret"],
    ];
    for args in attempts {
        let mut all = vec!["--otel", "sessions", "search"];
        all.extend_from_slice(args);
        let r = h.run(&all);
        assert_ne!(r.code, 0, "{args:?}");
        let spans: Vec<&str> = r.err.lines().filter(|l| l.starts_with("[otel] ")).collect();
        let joined = spans.join("\n");
        assert!(joined.contains("sessions search"), "{}", r.err);
        for secret in ["zebrasecret", "abcsecret", "quokkasecret"] {
            assert!(!joined.contains(secret), "{secret} in {joined}");
        }
    }
}
