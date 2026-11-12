//! `peektrace statusline`, run as a real process fed Claude Code's stdin
//! JSON (TS `statusline.test.ts`).

mod common;

use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{Home, Run, claude_lines, claude_path};
use serde_json::json;

const ID: &str = "44444444-4444-4444-8444-444444444444";
const BIG_ID: &str = "55555555-5555-4555-8555-555555555555";
const FALLBACK: &str = "ctx --\n";
/// Generous, so a slow CI machine never hits the deadline by accident.
const SLOW_OK: [&str; 3] = ["statusline", "--timeout", "60000"];

struct Fixture {
    home: Home,
    path: String,
    big: String,
}

fn fixture() -> Fixture {
    let home = Home::new("statusline");
    let path = home.write(
        &claude_path("-home-demo-work", ID),
        &claude_lines("2026-03-01T10:00:00.000Z", 50_000, "/home/demo/work"),
    );
    let big = home.write(
        &claude_path("-home-demo-work", BIG_ID),
        &claude_lines("2026-03-02T10:00:00.000Z", 150_000, "/home/demo/work"),
    );
    let s = |p: std::path::PathBuf| p.to_string_lossy().into_owned();
    Fixture {
        path: s(path),
        big: s(big),
        home,
    }
}

impl Fixture {
    fn run(&self, stdin: &str) -> Run {
        self.home.run_stdin(&SLOW_OK, stdin.as_bytes(), &[])
    }
}

fn ok(out: &str) -> Run {
    Run {
        code: 0,
        out: out.to_owned(),
        err: String::new(),
    }
}

#[test]
fn transcript_path_prints_percent_and_verdict() {
    let f = fixture();
    let r = f.run(&json!({ "transcript_path": f.path, "extra": 1 }).to_string());
    assert_eq!(r, ok("ctx 5% Healthy\n"));
}

#[test]
fn session_id_alone_resolves_the_same_session() {
    let f = fixture();
    assert_eq!(
        f.run(&json!({ "session_id": ID }).to_string()),
        ok("ctx 5% Healthy\n")
    );
}

#[test]
fn context_window_size_overrides_the_default_window() {
    let f = fixture();
    let narrow = f.run(
        &json!({ "transcript_path": f.big, "context_window": { "context_window_size": 200_000 } })
            .to_string(),
    );
    assert_eq!(narrow.out, "ctx 75% Rotting\n");
    let usual = f.run(&json!({ "transcript_path": f.big }).to_string());
    assert_eq!(usual.out, "ctx 15% Healthy\n");
}

#[test]
fn json_null_fields_count_as_absent() {
    let f = fixture();
    for payload in [
        json!({ "transcript_path": f.path, "context_window": null }),
        json!({ "transcript_path": f.path, "context_window": { "context_window_size": null } }),
        json!({ "transcript_path": f.path, "cwd": null, "session_id": null }),
        json!({ "transcript_path": null, "session_id": ID }),
    ] {
        assert_eq!(f.run(&payload.to_string()), ok("ctx 5% Healthy\n"));
    }
}

#[test]
fn empty_path_falls_back_to_session_id_and_a_short_id_is_ignored() {
    let f = fixture();
    let r = f.run(&json!({ "transcript_path": "", "session_id": ID }).to_string());
    assert_eq!(r.out, "ctx 5% Healthy\n");
    for short in ["4", "44444444"] {
        let r = f.run(&json!({ "session_id": short }).to_string());
        assert_eq!(r.out, FALLBACK, "{short}");
    }
}

#[test]
fn an_unresolvable_transcript_path_falls_back_to_session_id() {
    let f = fixture();
    for path in [format!("/elsewhere/{ID}.jsonl"), "/nope/missing.jsonl".to_owned()] {
        let r = f.run(&json!({ "session_id": ID, "transcript_path": path }).to_string());
        assert_eq!(r.out, "ctx 5% Healthy\n", "{path}");
    }
}

#[test]
fn a_wrongly_typed_context_window_loses_only_the_window() {
    let f = fixture();
    let r = f.run(
        &json!({ "transcript_path": f.path, "context_window": { "context_window_size": "200000" } })
            .to_string(),
    );
    assert_eq!(r.out, "ctx 5% Healthy\n");
}

/// Telemetry stays enabled here: `PEEKTRACE_NO_TELEMETRY` is removed.
#[test]
fn writes_no_telemetry_even_when_telemetry_is_enabled() {
    use std::io::Write as _;
    let f = fixture();
    let mut child = f
        .home
        .command(&SLOW_OK)
        .env_remove("PEEKTRACE_NO_TELEMETRY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let body = json!({ "transcript_path": f.path }).to_string();
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(Run::of(&out).out, "ctx 5% Healthy\n");
    assert!(!f.home.dir().join(".peektrace/telemetry.db").exists());
}

#[test]
fn bad_stdin_empty_object_and_unknown_id_fall_back_with_exit_0() {
    let f = fixture();
    for stdin in [
        "not json".to_owned(),
        "{}".to_owned(),
        String::new(),
        json!({ "session_id": "no-such-session" }).to_string(),
        json!({ "session_id": "99999999-9999-4999-8999-999999999999" }).to_string(),
    ] {
        assert_eq!(f.run(&stdin), ok(FALLBACK), "{stdin}");
    }
}

#[test]
fn a_bad_timeout_is_a_usage_error() {
    let f = fixture();
    let r = f
        .home
        .run_stdin(&["statusline", "--timeout", "1.5"], b"{}", &[]);
    assert_eq!(r.code, 1);
    assert_eq!(r.err, "'1.5' is not a integer\n\n");
    assert_eq!(r.out, "");
}

#[test]
fn stdin_held_open_times_out_falls_back_and_exits() {
    let f = fixture();
    let started = Instant::now();
    let mut child = f
        .home
        .command(&["statusline", "--timeout", "200"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let held = child.stdin.take();
    let out = child.wait_with_output().expect("wait");
    drop(held);
    assert_eq!(Run::of(&out), ok(FALLBACK));
    assert!(started.elapsed() < Duration::from_millis(2000 + 1500));
}
