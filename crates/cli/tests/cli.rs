//! End-to-end runs of the `peektrace` binary. Expected output was produced
//! by the TS CLI on the same files.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

const MTIME_MS: u64 = 1_794_415_341_851;
const MTIME_ISO: &str = "2026-11-11T16:42:21.851Z";

const S1: &str = r#"{"type":"user","timestamp":"2026-01-01T10:00:00.000Z","cwd":"","gitBranch":"main","message":{"role":"user","content":"hi"}}
{"type":"user","timestamp":"2026-01-01T10:00:01.000Z","cwd":"/tmp/proj","isMeta":true,"message":{"content":"meta"}}
{"type":"assistant","timestamp":"2026-01-01T10:00:02.000Z","requestId":"r1","message":{"model":"<synthetic>","usage":{"input_tokens":5}}}
{"type":"assistant","timestamp":"2026-01-01T10:00:03.000Z","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"cache_read_input_tokens":2000,"cache_creation_input_tokens":3000,"cache_creation":{"ephemeral_1h_input_tokens":1000},"output_tokens":10}}}
{"type":"assistant","timestamp":"2026-01-01T10:00:04.000Z","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-01-01T10:00:05.000Z","message":{"model":"mystery-model","usage":{"input_tokens":7}}}
{"type":"user","timestamp":"2026-01-01T10:00:06.000Z","message":{"content":[{"type":"tool_result","content":"x"}]}}
not json
{"type":"ai-title","aiTitle":"First title"}
{"type":"ai-title","aiTitle":"Final	title"}
{"type":"user","timestamp":"2026-01-01T09:00:00.000Z","gitBranch":"feat","message":{"content":"later line older ts"}}
{"type":"user","timestamp":"#;

const SUBAGENT_Y: &str = r#"{"type":"user","timestamp":"2026-01-01T10:00:02.500Z","isSidechain":true,"message":{"content":"look"}}
{"type":"assistant","timestamp":"2026-01-01T10:00:02.600Z","isSidechain":true,"requestId":"q1","message":{"model":"claude-opus-5-5","usage":{"input_tokens":400,"output_tokens":20},"content":[{"type":"text","text":"found it"}]}}
"#;

/// A throwaway HOME with one Claude project.
struct Fixture {
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!("peektrace-cli-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let proj = home.join(".claude/projects/-tmp-proj");
        fs::create_dir_all(proj.join("s1/subagents")).expect("mkdir");
        let write = |name: &str, body: &str| {
            let path = proj.join(name);
            fs::write(&path, body).expect("write");
            set_mtime(&path);
        };
        write("s1.jsonl", S1);
        write("s1/subagents/agent-x.jsonl", "{\"type\":\"user\"}\n");
        write("s1/subagents/agent-y.jsonl", SUBAGENT_Y);
        write(
            "s1/subagents/agent-y.meta.json",
            r#"{"agentType":"Explore","description":"Find the file"}"#,
        );
        write(
            "bad.jsonl",
            "{\"type\":\"user\",\"message\":{\"content\":\"x\"}}\nbad\n{\"type\":\"user\"}\n",
        );
        write("ws.jsonl", "\n  \n");
        write("empty.jsonl", "");
        write("notes.txt", "x");
        Self { home }
    }

    fn run(&self, args: &[impl AsRef<std::ffi::OsStr>]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PEEKTRACE_DIR", self.home.join(".peektrace"))
            .current_dir(&self.home)
            .output()
            .expect("run peektrace");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn set_mtime(path: &Path) {
    File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(UNIX_EPOCH + Duration::from_millis(MTIME_MS)))
        .expect("set mtime");
}

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

impl Fixture {
    fn settings(&self, body: &[u8]) -> PathBuf {
        let dir = self.home.join(".peektrace");
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("settings.json");
        fs::write(&path, body).expect("write settings");
        path
    }
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
fn unread_agents_are_named_when_they_have_sessions() {
    let f = Fixture::new();
    let (_, _, err) = f.run(&["sessions", "analyze", "--latest"]);
    assert_eq!(err, "");
    let (_, _, err) = f.run(&["sessions", "analyze", "zzz"]);
    assert_eq!(err, "Session \"zzz\" not found (searched all agents).\n");

    let opencode = f.home.join(".local/share/opencode");
    fs::create_dir_all(&opencode).expect("mkdir");
    fs::write(opencode.join("opencode.db"), "").expect("write");
    let note = "opencode sessions are not searched: not supported yet\n";
    for args in [
        &["sessions", "analyze", "--latest"][..],
        &["sessions", "analyze", "s"],
        &["sessions", "export", "--cwd", "/tmp", "--format", "json"],
    ] {
        let (code, _, err) = f.run(args);
        assert_eq!((code, err.as_str()), (0, note), "{args:?}");
    }
    let (code, _, err) = f.run(&["sessions", "analyze", "s1"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let (code, _, err) = f.run(&["sessions", "analyze", "zzz"]);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "Session \"zzz\" not found (searched claude, codex, pi; opencode not supported yet).\n"
    );
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

#[test]
fn codex_and_pi_sessions_list_and_analyze() {
    let f = Fixture::new();
    let codex = f.home.join(".codex/sessions/2026/02/01");
    fs::create_dir_all(&codex).expect("mkdir");
    let rollout =
        codex.join("rollout-2026-02-01T10-00-00-019f0000-0000-7000-8000-000000000001.jsonl");
    fs::write(
        &rollout,
        r#"{"timestamp":"2026-02-01T10:00:00.000Z","type":"session_meta","payload":{"id":"019f0000-0000-7000-8000-000000000001","cwd":"/tmp/demo"}}
{"timestamp":"2026-02-01T10:00:01.000Z","type":"turn_context","payload":{"model":"gpt-5.5","turn_id":"t"}}
{"timestamp":"2026-02-01T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}
{"timestamp":"2026-02-01T10:00:03.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":5},"model_context_window":258400}}}
"#,
    )
    .expect("write");
    let pi = f.home.join(".pi/agent/sessions/--tmp-demo--");
    fs::create_dir_all(&pi).expect("mkdir");
    fs::write(
        pi.join("2026-01-15_019f1111-0000-7000-8000-000000000001.jsonl"),
        r#"{"type":"session","id":"019f1111-0000-7000-8000-000000000001","timestamp":"2026-01-15T10:00:00.000Z","cwd":"/tmp/demo"}
{"type":"session_info","id":"i","parentId":null,"timestamp":"2026-01-15T10:00:01.000Z","name":"Pi title"}
"#,
    )
    .expect("write");

    let (code, out, err) = f.run(&["sessions", "ls", "--agent", "codex"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "id\tagent\tproject\tmodel\tmsgs\test cost\tsize\tupdated\ttitle\n\
         019f0000\tcodex\tdemo\tgpt-5.5\t1\t-\t612 B\t2026-02-01T10:00:03.000Z\t-\n"
    );
    let (code, out, _) = f.run(&["sessions", "ls", "--json", "--agent", "pi"]);
    assert_eq!(code, 0);
    let started = out.find("\"startedAt\"").expect("startedAt");
    assert!(out.find("\"title\"").is_some_and(|t| t > started), "{out}");

    let (code, out, err) = f.run(&["sessions", "analyze", "--cwd", "/tmp/demo"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.starts_with("field\tvalue\nsession\t019f0000-0000-7000-8000-000000000001\n"),
        "{out}"
    );
    let (code, out, _) = f.run(&["sessions", "export", "019f1111", "--format", "md"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("# Pi title\n"), "{out}");
    let (code, _, err) = f.run(&["sessions", "analyze", "019f"]);
    assert_eq!(code, 1);
    assert_eq!(err, "Session \"019f\" not found (searched all agents).\n");
}
