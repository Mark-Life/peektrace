//! `sessions ls / analyze / export` over Claude, Codex and Pi sessions, ported
//! from the TS CLI tests `sessions-ls.test.ts`, `sessions-analyze.test.ts`
//! and `sessions-export.test.ts`. Each test runs the binary over a throwaway
//! HOME, so nothing from the developer's machine leaks in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

const CLAUDE_ID: &str = "11111111-1111-4111-8111-111111111111";
const CODEX_ID: &str = "22222222-2222-4222-8222-222222222222";
const PI_ID: &str = "33333333-3333-4333-8333-333333333333";

const CLAUDE_CWD: &str = "/home/demo/work";
const PI_CWD: &str = "/home/demo/work/sub";
const CODEX_CWD: &str = "/home/demo/other";

const CLAUDE_AT: &str = "2026-03-01T10:00:00.000Z";
const PI_AT: &str = "2026-04-15T10:00:00.000Z";
const CODEX_AT: &str = "2026-06-01T10:00:00.000Z";

struct Home(PathBuf);

impl Home {
    fn new(prefix: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let home =
            std::env::temp_dir().join(format!("peektrace-{prefix}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).expect("mkdir");
        Self(home)
    }

    /// Write `lines` as JSONL at `rel` under HOME.
    fn write(&self, rel: &str, lines: &[Value]) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let body: String = lines.iter().map(|l| l.to_string() + "\n").collect();
        fs::write(path, body).expect("write");
    }

    fn codex(&self, id: &str, at: &str, cwd: &str) {
        self.write(
            &format!(".codex/sessions/2026/06/01/rollout-2026-06-01T10-00-00-{id}.jsonl"),
            &[
                json!({"timestamp": at, "type": "session_meta",
                    "payload": {"id": id, "timestamp": at, "cwd": cwd}}),
                json!({"timestamp": at, "type": "response_item", "payload": {
                    "type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "hello codex"}]}}),
            ],
        );
    }

    /// The clean env of the TS `envFor`.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let home: &Path = &self.0;
        let out = Command::new(env!("CARGO_BIN_EXE_peektrace"))
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("CLAUDE_CONFIG_DIR", home.join(".claude"))
            .env("CODEX_HOME", home.join(".codex"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("PEEKTRACE_DIR", home.join(".peektrace"))
            .env("PEEKTRACE_NO_TELEMETRY", "1")
            .current_dir(home)
            .output()
            .expect("run peektrace");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// sessions-ls.test.ts: one Claude, Codex and Pi session each (Codex newest,
// Claude oldest).

fn ls_home() -> Home {
    let h = Home::new("ls");
    h.write(
        &format!(".claude/projects/-home-demo-work/{CLAUDE_ID}.jsonl"),
        &[
            json!({"type": "user", "cwd": CLAUDE_CWD, "timestamp": CLAUDE_AT,
            "message": {"content": "hello claude"}}),
        ],
    );
    h.codex(CODEX_ID, CODEX_AT, CODEX_CWD);
    h.write(
        &format!(
            ".pi/agent/sessions/--home-demo-work-sub--/2026-04-15T10-00-00-000Z_{PI_ID}.jsonl"
        ),
        &[
            json!({"type": "session", "version": 3, "id": PI_ID, "timestamp": PI_AT,
                "cwd": PI_CWD}),
            json!({"type": "message", "id": "a1000000", "parentId": null, "timestamp": PI_AT,
                "message": {"role": "user", "content": [{"type": "text", "text": "hello pi"}]}}),
        ],
    );
    h
}

fn ls(h: &Home, args: &[&str]) -> (i32, String, String) {
    h.run(&[&["sessions", "ls"], args].concat())
}

fn ids(out: &str) -> Vec<String> {
    let rows: Vec<Value> = serde_json::from_str(out).expect("json rows");
    rows.iter()
        .map(|r| r["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// Data rows of the tab-separated table, header excluded.
fn data_rows(out: &str) -> Vec<Vec<&str>> {
    out.trim()
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect()
}

#[test]
fn no_agent_lists_every_agent_newest_first_with_a_short_id_agent_table() {
    let h = ls_home();
    let (code, out, _) = ls(&h, &[]);
    assert_eq!(code, 0);
    let head: Vec<_> = out.lines().next().unwrap_or_default().split('\t').collect();
    assert_eq!(head[..2], ["id", "agent"]);
    let rows = data_rows(&out);
    let agents: Vec<_> = rows.iter().map(|r| r[1]).collect();
    assert_eq!(agents, ["codex", "pi", "claude"]);
    assert_eq!(rows[0][0], &CODEX_ID[..8]);
}

#[test]
fn the_table_has_an_est_cost_column_dash_when_nothing_is_priced() {
    let h = ls_home();
    let (_, out, err) = ls(&h, &[]);
    let head: Vec<_> = out.lines().next().unwrap_or_default().split('\t').collect();
    let col = head.iter().position(|c| *c == "est cost").expect("column");
    assert_eq!(
        Some(col),
        head.iter().position(|c| *c == "msgs").map(|i| i + 1)
    );
    let cells: Vec<_> = data_rows(&out).iter().map(|r| r[col]).collect();
    assert_eq!(cells, ["-", "-", "-"]);
    assert!(!err.contains("EST COST covers"), "{err}");
}

#[test]
fn agent_narrows_to_one_agent_all_equals_no_agent() {
    let h = ls_home();
    let (code, out, _) = ls(&h, &["--agent", "pi", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(ids(&out), [PI_ID]);
    let (_, out, _) = ls(&h, &["--agent", "all", "--json"]);
    assert_eq!(ids(&out), [CODEX_ID, PI_ID, CLAUDE_ID]);
}

#[test]
fn limit_caps_rows_across_agents_and_hints_on_stderr() {
    let h = ls_home();
    let (code, out, err) = ls(&h, &["--limit", "1"]);
    assert_eq!(code, 0);
    assert_eq!(data_rows(&out).len(), 1);
    assert!(!out.contains("hidden"), "{out}");
    assert!(
        err.contains("2 more hidden, use --limit 0 to show all"),
        "{err}"
    );
    let (_, out, err) = ls(&h, &["--limit", "1", "--json"]);
    assert_eq!(ids(&out), [CODEX_ID]);
    assert!(
        err.contains("2 more hidden, use --limit 0 to show all"),
        "{err}"
    );
}

#[test]
fn since_keeps_only_sessions_updated_on_or_after_the_date() {
    let h = ls_home();
    let (_, out, _) = ls(&h, &["--since", "2026-05-01", "--json"]);
    assert_eq!(ids(&out), [CODEX_ID]);
}

#[test]
fn cwd_matches_the_directory_and_anything_below_it() {
    let h = ls_home();
    let (_, out, _) = ls(&h, &["--cwd", &format!("{CLAUDE_CWD}/"), "--json"]);
    assert_eq!(ids(&out), [PI_ID, CLAUDE_ID]);
}

// sessions-analyze.test.ts: two Claude sessions and one newer Codex session.

fn claude_lines(at: &str, context_tokens: u32, cwd: &str) -> [Value; 2] {
    [
        json!({"type": "user", "cwd": cwd, "timestamp": at, "message": {"content": "hello"}}),
        json!({"type": "assistant", "cwd": cwd, "timestamp": at, "requestId": "req-1",
            "message": {"model": "claude-opus-4-1",
                "content": [{"type": "text", "text": "hi"}],
                "usage": {"input_tokens": context_tokens, "cache_read_input_tokens": 0,
                    "cache_creation_input_tokens": 0, "output_tokens": 10}}}),
    ]
}

#[test]
fn analyze_latest_picks_the_newest_session_of_any_agent() {
    const MID_ID: &str = "22222222-2222-4222-8222-222222222222";
    const CODEX: &str = "33333333-3333-4333-8333-333333333333";
    let h = Home::new("analyze");
    h.write(
        &format!(".claude/projects/-home-demo-work/{CLAUDE_ID}.jsonl"),
        &claude_lines("2026-03-01T10:00:00.000Z", 20_000, "/home/demo/work"),
    );
    h.write(
        &format!(".claude/projects/-home-demo-work-sub/{MID_ID}.jsonl"),
        &claude_lines("2026-04-01T10:00:00.000Z", 40_000, "/home/demo/work/sub"),
    );
    h.write(
        &format!(".claude/projects/-home-demo-work-sub/{MID_ID}/subagents/agent-x.jsonl"),
        &claude_lines("2026-05-01T10:00:00.000Z", 99_000, "/home/demo/work/sub"),
    );
    h.codex(CODEX, CODEX_AT, CODEX_CWD);
    let (code, out, err) = h.run(&["sessions", "analyze", "--latest", "--json"]);
    assert_eq!(code, 0, "{err}");
    let a: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(a["sessionId"], CODEX);
}

// sessions-export.test.ts: one Claude session and one newer Codex session.

#[test]
fn export_latest_exports_a_non_claude_session() {
    const CODEX: &str = "33333333-3333-4333-8333-333333333333";
    let h = Home::new("export");
    h.write(
        &format!(".claude/projects/-home-demo-work/{CLAUDE_ID}.jsonl"),
        &claude_lines("2026-03-01T10:00:00.000Z", 1000, "/home/demo/work"),
    );
    h.codex(CODEX, CODEX_AT, CODEX_CWD);
    let (code, out, err) = h.run(&["sessions", "export", "--latest", "--format", "json"]);
    assert_eq!(code, 0, "{err}");
    let doc: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(doc["session"]["sessionId"], CODEX);
    assert_eq!(doc["session"]["provider"], "codex");
    assert!(!out.contains(&*h.0.to_string_lossy()), "{out}");
}

/// `--latest` reads the file the listing found; it does not walk every
/// agent's roots a second time to look the id up.
#[test]
fn latest_reads_the_listed_file_without_a_second_lookup() {
    let h = ls_home();
    let (code, out, err) = h.run(&["--otel", "sessions", "analyze", "--latest", "--json"]);
    assert_eq!(code, 0, "{err}");
    let a: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(a["sessionId"], CODEX_ID);
    let walks = err
        .lines()
        .filter(|l| l.starts_with("[otel] AgentRegistry.listSessionFiles "))
        .count();
    assert_eq!(walks, 3, "{err}");
    assert!(err.contains("[otel] Sessions.resolve "), "{err}");
}

#[test]
fn codex_and_pi_sessions_list_and_analyze() {
    let f = Home::new("codex-pi");
    let codex = f.0.join(".codex/sessions/2026/02/01");
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
    let pi = f.0.join(".pi/agent/sessions/--tmp-demo--");
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
