//! Projects, worktrees and parent threads as t3code's real schema has them.

use rusqlite::Connection;

use super::*;

/// The columns these reads use, in the shape of t3code's tables.
const PROJECTS: &str = "
CREATE TABLE projection_projects (project_id TEXT PRIMARY KEY, title TEXT NOT NULL,
    workspace_root TEXT NOT NULL, deleted_at TEXT);
INSERT INTO projection_projects VALUES
  ('pr1', 'peephole', '/code/peephole', NULL),
  ('pr2', 'Old', '/code/old', '2026-01-01'),
  ('pr3', 'No project', '/h/.t3/scratch', NULL);";

const V2: &str = "
CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL, title TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT,
    payload_json TEXT NOT NULL);
CREATE TABLE orchestration_v2_projection_provider_threads (provider_thread_id TEXT PRIMARY KEY,
    thread_id TEXT, updated_at TEXT NOT NULL, payload_json TEXT NOT NULL);";

/// `state.sqlite`, before the v2 tables.
const LEGACY: &str = "
CREATE TABLE projection_threads (thread_id TEXT PRIMARY KEY, project_id TEXT NOT NULL,
    title TEXT NOT NULL, branch TEXT, worktree_path TEXT, updated_at TEXT NOT NULL,
    deleted_at TEXT);
CREATE TABLE provider_session_runtime (thread_id TEXT PRIMARY KEY, provider_name TEXT,
    resume_cursor_json TEXT);";

fn index(name: &str, ddl: &str, sql: &str) -> Index {
    let dir = std::env::temp_dir().join(format!("peektrace-t3wt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("statev2.sqlite").to_string_lossy().into_owned();
    Connection::open(&path)
        .and_then(|c| c.execute_batch(&format!("{PROJECTS}{ddl}{sql}")))
        .expect("fixture db");
    read_index(&path).expect("read")
}

fn known(project: &str, root: &str, branch: Option<&str>) -> Known {
    Known {
        project: project.to_owned(),
        project_root: Some(root.to_owned()),
        branch: branch.map(Into::into),
    }
}

#[test]
fn worktree_paths_name_their_project_and_branch() {
    let index = index(
        "current",
        V2,
        r#"
INSERT INTO orchestration_v2_projection_threads VALUES
  ('t1', 'pr1', 'Root thread', '1', NULL, '{}'),
  ('t2', 'pr1', 'Child', '2', NULL,
   '{"branch":"t3/rows","worktreePath":"/h/.t3/worktrees/peephole/t3-a/",
     "lineage":{"parentThreadId":"t1","relationshipToParent":"subagent","rootThreadId":"t1"}}'),
  ('t3', 'pr1', 'Older on the same path', '0', NULL,
   '{"branch":"stale","worktreePath":"/h/.t3/worktrees/peephole/t3-a"}'),
  ('t4', 'pr2', 'Deleted', '9', '2026-01-02',
   '{"branch":"gone","worktreePath":"/h/.t3/worktrees/peephole/t3-a"}'),
  ('t5', 'pr9', 'No project row', '1', NULL, '{"worktreePath":"/h/x"}'),
  ('t6', 'pr3', 'Scratch', '1', NULL, '{"worktreePath":"/h/.t3/scratch/2026-11-14-a"}');
INSERT INTO orchestration_v2_projection_provider_threads VALUES
  ('p2', 't2', '1', '{"nativeThreadRef":{"driver":"claudeAgent","nativeId":"c-2"}}');
"#,
    );
    assert_eq!(
        index.worktree("/h/.t3/worktrees/peephole/t3-a"),
        Some(known("peephole", "/code/peephole", Some("t3/rows")))
    );
    assert_eq!(index.worktree("/h/x"), None);
    assert_eq!(index.worktree("/h/.t3/scratch/2026-11-14-a"), None);
    let child = index.get(Agent::Claude, "c-2").expect("child");
    assert_eq!(child.launcher.parent_title.as_deref(), Some("Root thread"));
    assert_eq!(child.launcher.branch.as_deref(), Some("t3/rows"));
}

#[test]
fn deleted_or_unnamed_parents_give_no_title() {
    let index = index(
        "parents",
        V2,
        r#"
INSERT INTO orchestration_v2_projection_threads VALUES
  ('gone', 'pr1', 'Gone parent', '1', '2026-01-01', '{}'),
  ('fresh', 'pr1', 'New thread', '1', NULL, '{}'),
  ('a', 'pr1', 'A', '1', NULL, '{"lineage":{"parentThreadId":"gone"}}'),
  ('b', 'pr1', 'B', '1', NULL, '{"lineage":{"parentThreadId":"fresh"}}'),
  ('c', 'pr1', 'C', '1', NULL, '{"lineage":{"parentThreadId":"missing"}}');
INSERT INTO orchestration_v2_projection_provider_threads VALUES
  ('pa', 'a', '1', '{"nativeThreadRef":{"driver":"claudeAgent","nativeId":"a"}}'),
  ('pb', 'b', '1', '{"nativeThreadRef":{"driver":"claudeAgent","nativeId":"b"}}'),
  ('pc', 'c', '1', '{"nativeThreadRef":{"driver":"codex","nativeId":"c"}}');
"#,
    );
    for (agent, id) in [
        (Agent::Claude, "a"),
        (Agent::Claude, "b"),
        (Agent::Codex, "c"),
    ] {
        let t = index.get(agent, id).expect(id);
        assert_eq!(t.launcher.parent_title, None, "{id}");
    }
}

#[test]
fn legacy_threads_name_their_worktree_project() {
    let index = index(
        "legacy",
        LEGACY,
        "
INSERT INTO projection_threads VALUES
  ('t1', 'pr1', 'Legacy', 'feat/old', '/h/.t3/worktrees/peephole/t3-b', '1', NULL);
INSERT INTO provider_session_runtime VALUES
  ('t1', 'claudeAgent', '{\"resume\":\"c-1\"}');",
    );
    assert_eq!(
        index.worktree("/h/.t3/worktrees/peephole/t3-b"),
        Some(known("peephole", "/code/peephole", Some("feat/old")))
    );
    let t = index.get(Agent::Claude, "c-1").expect("legacy thread");
    assert_eq!(t.launcher.parent_title, None);
}

#[test]
fn headers_take_the_worktree_project_by_cwd_or_thread_path() {
    let mut idx = Index::default();
    idx.worktrees.insert(
        "/h/.t3/worktrees/peephole/t3-a".into(),
        known("peephole", "/code/peephole", Some("t3/rows")),
    );
    let by_cwd = worktree_of(&idx, Some("/h/.t3/worktrees/peephole/t3-a/"), None, None);
    assert_eq!(
        by_cwd.map(|w| w.project_root),
        Some(Some("/code/peephole".into()))
    );
    let launcher = Launcher {
        name: "t3code".into(),
        branch: None,
        worktree_path: Some("/h/.t3/worktrees/peephole/t3-a".into()),
        parent_title: None,
    };
    let by_thread = worktree_of(&idx, Some("/elsewhere"), Some(&launcher), None);
    assert_eq!(by_thread.and_then(|w| w.branch), Some("t3/rows".into()));
    // No record: the `<t3 home>/worktrees` layout still names the project.
    let by_path = worktree_of(
        &Index::default(),
        Some("/h/.t3/worktrees/other/t3-c"),
        None,
        Some("/h/.t3/userdata"),
    );
    assert_eq!(by_path.map(|w| w.project), Some("other".into()));
    assert_eq!(t3_home(Some("/h/.t3/userdata/")), Some("/h/.t3"));
}
