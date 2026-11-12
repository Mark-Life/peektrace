//! End-to-end runs of the `peektrace` binary. Expected output was produced
//! by the TS CLI on the same files.

mod common;

use std::fs;
use std::process::Command;

use common::{Fixture, MTIME_ISO};

#[test]
fn compact_table_and_notes() {
    let f = Fixture::new();
    let (code, out, err) = f.run(&["sessions", "ls", "--agent", "claude", "--limit", "0"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!(
            "id\tagent\tproject\tmodel\tmsgs\test cost\tsize\tupdated\ttitle\n\
             bad\tclaude\t-tmp-proj\t-\t2\t-\t62 B\t{MTIME_ISO}\t-\n\
             ws\tclaude\t-tmp-proj\t-\t0\t-\t4 B\t{MTIME_ISO}\t-\n\
             s1\tclaude\t-tmp-proj\tclaude-opus-5-5\t5\t~$0.02*\t1.3 KB\t2026-01-01T09:00:00.000Z\tFirst title\n"
        )
    );
    assert_eq!(
        err,
        "EST COST covers the main transcript only, without subagent files\n\
         * some models unpriced: the figure is too low\n"
    );
}

#[test]
fn json_rows() {
    let f = Fixture::new();
    let (code, out, err) = f.run(&[
        "sessions",
        "ls",
        "--json",
        "--limit",
        "1",
        "--since",
        "2026-01-01",
    ]);
    assert_eq!(code, 0);
    let path = f.home.join(".claude/projects/-tmp-proj/bad.jsonl");
    assert_eq!(
        out,
        format!(
            "[\n  {{\n    \"id\": \"bad\",\n    \"agent\": \"claude\",\n    \"path\": \"{}\",\n    \"project\": \"-tmp-proj\",\n    \"messageCount\": 2,\n    \"sizeBytes\": 62,\n    \"updatedAt\": \"{MTIME_ISO}\",\n    \"malformedLines\": 1\n  }}\n]\n",
            path.display()
        )
    );
    assert_eq!(
        err,
        "2 more hidden, use --limit 0 to show all\n\
         opencode sessions are not listed: not supported yet\n"
    );
}

#[test]
fn pretty_table() {
    let f = Fixture::new();
    let (code, out, _) = f.run(&["--pretty", "sessions", "ls", "--cwd", "/tmp"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "ID  AGENT   PROJECT    MODEL            MSGS  EST COST  SIZE    UPDATED                   TITLE\n\
         --  ------  ---------  ---------------  ----  --------  ------  ------------------------  -----------\n\
         s1  claude  -tmp-proj  claude-opus-5-5  5     ~$0.02*   1.3 KB  2026-01-01T09:00:00.000Z  First title\n"
    );
}

#[test]
fn empty_results() {
    let f = Fixture::new();
    let (code, out, err) = f.run(&["sessions", "ls", "--agent", "claude", "--project=nope"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "id\tagent\tproject\tmodel\tmsgs\test cost\tsize\tupdated\ttitle\n(none)\n"
    );
}

/// Node decodes `argv` lossily, so a non-UTF-8 path still runs.
#[cfg(unix)]
#[test]
fn non_utf8_flag_values_run() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    let f = Fixture::new();
    let bad = OsStr::from_bytes(b"/tmp/a\xff");
    let args = [
        OsStr::new("--otel"),
        OsStr::new("sessions"),
        OsStr::new("ls"),
        OsStr::new("--agent"),
        OsStr::new("claude"),
        OsStr::new("--cwd"),
        bad,
        OsStr::new("--project"),
        bad,
    ];
    let (code, out, err) = f.run(&args);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "id\tagent\tproject\tmodel\tmsgs\test cost\tsize\tupdated\ttitle\n(none)\n"
    );
    assert!(err.contains("/tmp/a\u{FFFD}"), "{err}");
}

#[test]
fn user_errors_exit_1() {
    let f = Fixture::new();
    let cases: [(&[&str], &str); 5] = [
        (
            &["sessions", "ls", "--limit", "abc"],
            "'abc' is not a integer\n\n",
        ),
        (
            &["sessions", "ls", "--limit", "-1"],
            "Invalid --limit -1. Use 0 for all, or a positive number.\n",
        ),
        (
            &["sessions", "ls", "--agent", "bogus"],
            "Unknown agent \"bogus\". Known: claude, codex, pi, opencode, all.\n",
        ),
        (
            &["sessions", "ls", "--json", "--agent", "opencode"],
            "Agent \"opencode\" is not supported yet. Supported: claude, codex, pi.\n",
        ),
        (
            &["sessions", "ls", "--since", "xx"],
            "Invalid --since \"xx\". Use an ISO date (2026-01-31) or a relative value (7d, 12h, 2w).\n",
        ),
    ];
    for (args, want) in cases {
        let (code, out, err) = f.run(args);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (1, "", want),
            "{args:?}"
        );
    }
    let (code, _, _) = f.run(&["sessions", "ls", "--bogus"]);
    assert_eq!(code, 1);
}

#[test]
fn version_flag() {
    let f = Fixture::new();
    assert_eq!(f.run(&["-v"]), (0, "0.0.0-dev\n".to_owned(), String::new()));
}

#[test]
fn otel_echo_lists_spans() {
    let f = Fixture::new();
    let (code, _, err) = f.run(&["--otel", "sessions", "ls", "--agent", "claude"]);
    assert_eq!(code, 0);
    let names: Vec<_> = err
        .lines()
        .filter_map(|l| l.strip_prefix("[otel] "))
        .filter_map(|l| l.split(' ').next())
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|n| **n == "AgentRegistry.loadTranscript")
            .count(),
        4
    );
    assert!(err.contains("Sessions.list"));
    assert!(err.contains(r#"{"agent":"claude","malformedLines":3}"#));
    assert!(err.contains(r#""sessions.unreadable":0"#));
    assert_eq!(names.last(), Some(&"cli"));
}

#[test]
fn unparseable_settings_warn() {
    let f = Fixture::new();
    let path = f.settings(b"{}");
    let warn = format!(
        "WARN peektrace: ignoring unparseable settings at {}\n",
        path.display()
    );
    let (code, _, err) = f.run(&["sessions", "ls", "--agent", "claude", "--json"]);
    assert_eq!((code, err.as_str()), (0, warn.as_str()));
    let (_, _, err) = f.run(&[
        "sessions",
        "ls",
        "--agent",
        "claude",
        "--json",
        "--log-level",
        "error",
    ]);
    assert_eq!(err, "");
    f.settings(br#"{"roots":{}}"#);
    let (_, _, err) = f.run(&["sessions", "ls", "--agent", "claude", "--json"]);
    assert_eq!(err, "");
}

#[test]
fn settings_with_invalid_utf8_keep_roots() {
    let f = Fixture::new();
    let extra = f.home.join("work/.claude/projects/-w");
    fs::create_dir_all(&extra).expect("mkdir");
    fs::write(extra.join("w1.jsonl"), "{\"type\":\"user\"}\n").expect("write");
    let mut body = br#"{"roots":{"claude":[{"path":"~/work/.claude","label":"w"#.to_vec();
    body.extend_from_slice(b"\xffk\"}]}}");
    f.settings(&body);
    let (code, out, _) = f.run(&["sessions", "ls", "--json", "--project", "-w"]);
    assert_eq!(code, 0);
    assert!(out.contains("\"source\": \"claude:w\u{FFFD}k\""), "{out}");
}

#[test]
fn settings_with_lone_surrogate_keep_roots() {
    let f = Fixture::new();
    let extra = f.home.join("work/.claude/projects/-w");
    fs::create_dir_all(&extra).expect("mkdir");
    fs::write(extra.join("w1.jsonl"), "{\"type\":\"user\"}\n").expect("write");
    f.settings(br#"{"roots":{"claude":[{"path":"~/work/.claude","label":"w\ud800"}]}}"#);
    let (code, out, err) = f.run(&[
        "sessions",
        "ls",
        "--agent",
        "claude",
        "--json",
        "--project",
        "-w",
    ]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains(r#""sourceLabel": "w\ud800""#), "{out}");
}

#[cfg(unix)]
#[test]
fn early_failure_marks_cli_span_failed() {
    let f = Fixture::new();
    let gone = f.home.join("gone-otel");
    fs::create_dir_all(&gone).expect("mkdir");
    let out = Command::new("sh")
        .args([
            "-c",
            r#"cd "$1" && rmdir "$1" && exec "$2" --otel sessions ls --cwd foo"#,
            "sh",
        ])
        .arg(&gone)
        .arg(env!("CARGO_BIN_EXE_peektrace"))
        .env_clear()
        .env("HOME", &f.home)
        .env("PEEKTRACE_DIR", f.home.join(".peektrace"))
        .output()
        .expect("run peektrace");
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    let cli = err
        .lines()
        .find(|l| l.starts_with("[otel] cli "))
        .unwrap_or_default();
    assert!(cli.contains("ms fail "), "{err}");
    assert!(!cli.contains(r#""error""#), "{err}");
}

#[cfg(unix)]
#[test]
fn deleted_working_dir_is_fine_without_relative_cwd() {
    let f = Fixture::new();
    let gone = f.home.join("gone");
    fs::create_dir_all(&gone).expect("mkdir");
    let out = Command::new("sh")
        .args([
            "-c",
            r#"cd "$1" && rmdir "$1" && exec "$2" sessions ls --limit 1"#,
            "sh",
        ])
        .arg(&gone)
        .arg(env!("CARGO_BIN_EXE_peektrace"))
        .env_clear()
        .env("HOME", &f.home)
        .env("PEEKTRACE_DIR", f.home.join(".peektrace"))
        .output()
        .expect("run peektrace");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("-tmp-proj"));
}

#[cfg(unix)]
#[test]
fn unsearchable_project_dir_is_skipped() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new();
    let dir = f.home.join(".claude/projects/-q");
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("b.jsonl"), "{\"type\":\"user\"}\n").expect("write");
    let mode = |m| fs::set_permissions(&dir, fs::Permissions::from_mode(m)).expect("chmod");
    mode(0o444);
    let (code, out, err) = f.run(&["sessions", "ls", "--agent", "claude", "--limit", "0"]);
    mode(0o755);
    assert_eq!(code, 0);
    assert!(!out.contains("-q"), "{out}");
    assert!(!err.contains("could not be read"), "{err}");
}

#[cfg(unix)]
#[test]
fn root_alias_is_scanned_once() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(f.home.join(".claude"), f.home.join("alias")).expect("symlink");
    f.settings(br#"{"roots":{"claude":[{"path":"~/alias","label":"alias"}]}}"#);
    let (code, out, _) = f.run(&["sessions", "ls", "--json", "--limit", "0"]);
    assert_eq!(code, 0);
    assert_eq!(out.matches("\"id\": \"s1\"").count(), 1, "{out}");
}

#[test]
fn bare_sessions_prints_help() {
    let f = Fixture::new();
    let (code, out, err) = f.run(&["sessions"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("Usage: peektrace sessions"), "{out}");
}

#[cfg(unix)]
#[test]
fn closed_stdout_exits_0() {
    let f = Fixture::new();
    let cases: [&[&str]; 4] = [
        &[],
        &["sessions"],
        &["--help"],
        &["sessions", "ls", "--limit", "0"],
    ];
    for args in cases {
        let (reader, writer) = std::io::pipe().expect("pipe");
        drop(reader);
        let status = Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .args(args)
            .env_clear()
            .env("HOME", &f.home)
            .env("PEEKTRACE_DIR", f.home.join(".peektrace"))
            .stdout(writer)
            .status()
            .expect("run peektrace");
        assert_eq!(status.code(), Some(0), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn unreadable_transcript_span_fails_without_error_attr() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new();
    let file = f.home.join(".claude/projects/-tmp-proj/locked.jsonl");
    fs::write(&file, "{\"type\":\"user\"}\n").expect("write");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("chmod");
    let (code, _, err) = f.run(&["--otel", "sessions", "ls", "--agent", "claude"]);
    assert_eq!(code, 0);
    let failed: Vec<_> = err
        .lines()
        .filter(|l| l.starts_with("[otel] AgentRegistry.loadTranscript ") && l.contains("ms fail"))
        .collect();
    assert_eq!(failed.len(), 1, "{err}");
    assert!(
        failed[0].ends_with(r#"ms fail {"agent":"claude"}"#),
        "{err}"
    );
}
