//! `sessions ls` lists a session from a linked worktree under its main
//! checkout's project, and `--project` finds it there.

mod common;

use common::{Home, claude_lines, claude_path, codex_lines, codex_path};
use serde_json::Value;

const MAIN_ID: &str = "aaaaaaaa-1111-4111-8111-111111111111";
const WT_ID: &str = "cccccccc-3333-4333-8333-333333333333";
const CODEX_ID: &str = "bbbbbbbb-2222-4222-8222-222222222222";
const WT: &str = "/h/.t3/worktrees/app/t3-1b686728";

/// One Claude session in the main checkout `/code/app`, and one Claude and
/// one Codex session in a t3code worktree of it.
fn home() -> Home {
    let h = Home::new("worktrees");
    h.write(
        &claude_path("-code-app", MAIN_ID),
        &claude_lines("2026-01-01T10:00:00.000Z", 1000, "/code/app"),
    );
    h.write(
        &claude_path("-h--t3-worktrees-app-t3-1b686728", WT_ID),
        &claude_lines("2026-01-01T11:00:00.000Z", 1000, WT),
    );
    h.write(&codex_path(CODEX_ID), &codex_lines(CODEX_ID, WT));
    let dir = h.dir().join(".t3/userdata");
    std::fs::create_dir_all(&dir).expect("mkdir");
    rusqlite::Connection::open(dir.join("statev2.sqlite"))
        .and_then(|c| {
            c.execute_batch(&format!(
                r#"
CREATE TABLE projection_projects (project_id TEXT PRIMARY KEY, title TEXT NOT NULL,
    workspace_root TEXT NOT NULL, deleted_at TEXT);
CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL, title TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT,
    payload_json TEXT NOT NULL);
CREATE TABLE orchestration_v2_projection_provider_threads (provider_thread_id TEXT PRIMARY KEY,
    thread_id TEXT, updated_at TEXT NOT NULL, payload_json TEXT NOT NULL);
INSERT INTO projection_projects VALUES ('pr1', 'App', '/code/app', NULL);
INSERT INTO orchestration_v2_projection_threads VALUES
  ('root', 'pr1', 'Plan the release', '1', NULL, '{{}}'),
  ('t1', 'pr1', 'Ship rows', '2', NULL,
   '{{"branch":"t3/rows","worktreePath":"{WT}","lineage":{{"parentThreadId":"root"}}}}');
INSERT INTO orchestration_v2_projection_provider_threads VALUES
  ('p1', 't1', '1', '{{"nativeThreadRef":{{"driver":"claudeAgent","nativeId":"{WT_ID}"}}}}');
"#
            ))
        })
        .expect("fixture db");
    h
}

/// The id prefix and project cell of each printed row.
fn rows(out: &str) -> Vec<(String, String)> {
    out.lines()
        .skip(1)
        .filter_map(|r| {
            let cells: Vec<&str> = r.split('\t').collect();
            Some((cells.first()?.to_string(), cells.get(2)?.to_string()))
        })
        .collect()
}

#[test]
fn worktree_rows_print_and_match_their_main_checkout() {
    let h = home();
    let all = h.run(&["sessions", "ls"]);
    assert_eq!(all.code, 0, "{}", all.err);
    let cells = rows(&all.out);
    assert_eq!(cells.len(), 3, "{}", all.out);
    for (id, want) in [
        ("aaaa", "-code-app"),
        ("cccc", "-code-app"),
        ("bbbb", "app"),
    ] {
        let cell = cells.iter().find(|(i, _)| i.starts_with(id));
        assert_eq!(cell.map(|(_, p)| p.as_str()), Some(want), "{}", all.out);
    }

    let claude = h.run(&["sessions", "ls", "--project", "-code-app"]);
    let mut ids: Vec<String> = rows(&claude.out).into_iter().map(|(i, _)| i).collect();
    ids.sort();
    assert_eq!(ids.len(), 2, "{}", claude.out);
    assert!(
        ids[0].starts_with("aaaa") && ids[1].starts_with("cccc"),
        "{ids:?}"
    );

    let codex = h.run(&["sessions", "ls", "--project", "app"]);
    let ids: Vec<String> = rows(&codex.out).into_iter().map(|(i, _)| i).collect();
    assert_eq!(ids.len(), 1, "{}", codex.out);
    assert!(ids[0].starts_with("bbbb"), "{ids:?}");

    for (own, id) in [
        ("-h--t3-worktrees-app-t3-1b686728", "cccc"),
        ("t3-1b686728", "bbbb"),
    ] {
        let run = h.run(&["sessions", "ls", "--project", own]);
        let ids: Vec<String> = rows(&run.out).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ids.len(), 1, "{own}: {}", run.out);
        assert!(ids[0].starts_with(id), "{own}: {ids:?}");
    }
}

#[test]
fn json_rows_carry_the_worktree_and_parent_thread() {
    let h = home();
    let run = h.run(&["sessions", "ls", "--json"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let v: Vec<Value> = serde_json::from_str(&run.out).expect("json");
    let row = v.iter().find(|r| r["id"] == WT_ID).expect("worktree row");
    assert_eq!(
        row["worktree"],
        serde_json::json!({"project": "App", "projectRoot": "/code/app", "path": WT, "branch": "t3/rows"})
    );
    assert_eq!(row["launcher"]["parentTitle"], "Plan the release");
    assert_eq!(row["project"], "-h--t3-worktrees-app-t3-1b686728");
    assert_eq!(
        row["modelProvider"],
        serde_json::json!({"id": "anthropic", "name": "Anthropic"})
    );
    let codex = v.iter().find(|r| r["id"] == CODEX_ID).expect("codex row");
    assert_eq!(codex["worktree"]["project"], "App");
    assert_eq!(codex["launcher"], Value::Null);
    let main = v.iter().find(|r| r["id"] == MAIN_ID).expect("main row");
    assert_eq!(main["worktree"], Value::Null);
}
