//! `sessions ls` and `sessions analyze` name a session t3code started after
//! its thread, and mark t3code as its launcher.

mod common;

use common::{Home, claude_lines, claude_path, codex_lines, codex_path};
use serde_json::Value;

const CLAUDE_ID: &str = "aaaaaaaa-1111-4111-8111-111111111111";
const CODEX_ID: &str = "bbbbbbbb-2222-4222-8222-222222222222";

fn home() -> Home {
    let h = Home::new("t3code");
    h.write(
        &claude_path("-w", CLAUDE_ID),
        &claude_lines("2026-01-01T10:00:00.000Z", 1000, "/w"),
    );
    h.write(&codex_path(CODEX_ID), &codex_lines(CODEX_ID, "/w"));
    let dir = h.dir().join(".t3/userdata");
    std::fs::create_dir_all(&dir).expect("mkdir");
    rusqlite::Connection::open(dir.join("statev2.sqlite"))
        .and_then(|c| {
            c.execute_batch(&format!(
                r#"
CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY, title TEXT,
    payload_json TEXT, archived_at TEXT, deleted_at TEXT);
CREATE TABLE orchestration_v2_projection_provider_threads (provider_thread_id TEXT PRIMARY KEY,
    thread_id TEXT, updated_at TEXT, payload_json TEXT);
INSERT INTO orchestration_v2_projection_threads VALUES
  ('t1', 'Claude thread', '{{"branch":"feat/a"}}', NULL, NULL),
  ('t2', 'Codex thread', '{{}}', '2026-01-02', NULL);
INSERT INTO orchestration_v2_projection_provider_threads VALUES
  ('p1', 't1', '1', '{{"nativeThreadRef":{{"driver":"claudeAgent","nativeId":"{CLAUDE_ID}"}}}}'),
  ('p2', 't2', '1', '{{"nativeThreadRef":{{"driver":"codex","nativeId":"{CODEX_ID}"}}}}');
"#
            ))
        })
        .expect("fixture db");
    h
}

#[test]
fn ls_shows_the_thread_name_and_launcher() {
    let h = home();
    let run = h.run(&["sessions", "ls"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let rows: Vec<&str> = run.out.lines().collect();
    assert!(
        rows.iter()
            .any(|r| r.contains("\tclaude (t3code)\t") && r.ends_with("\tClaude thread")),
        "{}",
        run.out
    );
    assert!(
        rows.iter()
            .any(|r| r.contains("\tcodex (t3code)\t") && r.ends_with("\tCodex thread")),
        "{}",
        run.out
    );

    let run = h.run(&["sessions", "ls", "--json"]);
    let v: Vec<Value> = serde_json::from_str(&run.out).expect("json");
    let claude = v.iter().find(|r| r["id"] == CLAUDE_ID).expect("claude row");
    assert_eq!(
        claude["launcher"],
        serde_json::json!({"name": "t3code", "branch": "feat/a"})
    );
}

#[test]
fn analyze_takes_the_thread_name() {
    let h = home();
    let run = h.run(&["sessions", "analyze", CLAUDE_ID, "--json"]);
    assert_eq!((run.code, run.err.as_str()), (0, ""));
    let v: Value = serde_json::from_str(&run.out).expect("json");
    assert_eq!(v["title"], "Claude thread");
    assert_eq!(v["launcher"]["name"], "t3code");
}
