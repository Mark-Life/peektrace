use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::Connection;

use super::*;

const V2_DDL: &str = "
CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY, title TEXT,
    payload_json TEXT, archived_at TEXT, deleted_at TEXT);
CREATE TABLE orchestration_v2_projection_provider_threads (provider_thread_id TEXT PRIMARY KEY,
    thread_id TEXT, updated_at TEXT, payload_json TEXT);";

const LEGACY_DDL: &str = "
CREATE TABLE projection_threads (thread_id TEXT PRIMARY KEY, title TEXT, branch TEXT,
    worktree_path TEXT, archived_at TEXT, deleted_at TEXT);
CREATE TABLE provider_session_runtime (thread_id TEXT PRIMARY KEY, provider_name TEXT,
    resume_cursor_json TEXT);";

/// A fresh empty dir under the system temp dir.
fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("peektrace-t3-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn db(dir: &std::path::Path, name: &str, sql: &str) -> String {
    let path = dir.join(name).to_string_lossy().into_owned();
    Connection::open(&path)
        .and_then(|c| c.execute_batch(sql))
        .expect("fixture db");
    path
}

/// Move a file's mtime a minute ahead, so the cache sees a change even when
/// the write kept its size and landed in the same clock tick.
fn bump(path: &str) {
    let m = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .expect("mtime");
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(m + Duration::from_secs(60)))
        .expect("touch");
}

fn thread(title: &str, branch: Option<&str>, worktree: Option<&str>) -> Thread {
    Thread {
        title: Some(title.to_owned()),
        launcher: Launcher {
            name: "t3code".into(),
            branch: branch.map(Into::into),
            worktree_path: worktree.map(Into::into),
            parent_title: None,
        },
    }
}

#[test]
fn v2_threads_map_native_ids_to_titles() {
    let dir = temp_dir("v2");
    let path = db(
        &dir,
        "statev2.sqlite",
        &format!(
            "{V2_DDL}
            INSERT INTO orchestration_v2_projection_threads VALUES
              ('t1', 'Fix the parser', '{{\"branch\":\"feat/x\",\"worktreePath\":\"/w/x\"}}', NULL, NULL),
              ('t2', 'Gone', '{{}}', NULL, '2026-01-02'),
              ('t3', 'Kept', '{{}}', '2026-01-03', NULL),
              ('t4', 'New thread', '{{}}', NULL, NULL);
            INSERT INTO orchestration_v2_projection_provider_threads VALUES
              ('p1', 't1', '1', '{{\"nativeThreadRef\":{{\"driver\":\"claudeAgent\",\"nativeId\":\"c-1\"}}}}'),
              ('p2', 't2', '1', '{{\"nativeThreadRef\":{{\"driver\":\"claudeAgent\",\"nativeId\":\"c-2\"}}}}'),
              ('p3', 't3', '1', '{{\"nativeThreadRef\":{{\"driver\":\"codex\",\"nativeId\":\"x-3\"}}}}'),
              ('p4', 't1', '1', '{{\"nativeThreadRef\":null}}'),
              ('p5', 't1', '1', '{{\"nativeThreadRef\":{{\"driver\":\"other\",\"nativeId\":\"o-5\"}}}}'),
              ('p6', 't4', '1', '{{\"nativeThreadRef\":{{\"driver\":\"claudeAgent\",\"nativeId\":\"c-4\"}}}}');"
        ),
    );
    let index = read_index(&path).expect("read");
    assert_eq!(index.threads.len(), 4);
    assert_eq!(
        index.get(Agent::Claude, "c-4").map(|t| &t.title),
        Some(&None)
    );
    assert_eq!(
        index.get(Agent::Claude, "c-1"),
        Some(&thread("Fix the parser", Some("feat/x"), Some("/w/x")))
    );
    assert_eq!(
        index.get(Agent::Claude, "c-2").map(|t| &t.title),
        Some(&None)
    );
    assert_eq!(
        index.get(Agent::Codex, "x-3"),
        Some(&thread("Kept", None, None))
    );
    assert_eq!(index.get(Agent::Claude, "x-3"), None);
}

#[test]
fn legacy_cursors_take_current_titles_when_both_tables_exist() {
    let dir = temp_dir("legacy");
    let path = db(
        &dir,
        "statev2.sqlite",
        &format!(
            "{LEGACY_DDL}{V2_DDL}
            INSERT INTO projection_threads VALUES ('t1', 'Old name', 'main', NULL, NULL, NULL);
            INSERT INTO provider_session_runtime VALUES
              ('t1', 'claudeAgent', '{{\"threadId\":\"t1\",\"resume\":\"c-1\"}}');
            INSERT INTO orchestration_v2_projection_threads VALUES
              ('t1', 'New name', '{{\"branch\":\"main\"}}', NULL, NULL);"
        ),
    );
    let index = read_index(&path).expect("read");
    assert_eq!(
        index
            .get(Agent::Claude, "c-1")
            .and_then(|t| t.title.as_deref()),
        Some("New name")
    );
    assert_eq!(index.get(Agent::Claude, "t1"), None);
}

#[test]
fn legacy_db_maps_claude_resume_and_codex_thread_ids() {
    let dir = temp_dir("legacy-only");
    let path = db(
        &dir,
        "state.sqlite",
        &format!(
            "{LEGACY_DDL}
            INSERT INTO projection_threads VALUES
              ('t1', 'Claude run', 'main', '/w/a', NULL, NULL),
              ('t2', 'Codex run', NULL, NULL, NULL, '2026-01-01');
            INSERT INTO provider_session_runtime VALUES
              ('t1', 'claudeAgent', '{{\"threadId\":\"t1\",\"resume\":\"c-1\"}}'),
              ('t2', 'codex', '{{\"threadId\":\"x-2\"}}');"
        ),
    );
    let index = read_index(&path).expect("read");
    assert_eq!(
        index.get(Agent::Claude, "c-1"),
        Some(&thread("Claude run", Some("main"), Some("/w/a")))
    );
    assert_eq!(
        index.get(Agent::Codex, "x-2").map(|t| &t.title),
        Some(&None)
    );
}

#[test]
fn unknown_schema_or_missing_file_reads_nothing() {
    let dir = temp_dir("odd");
    let path = db(&dir, "statev2.sqlite", "CREATE TABLE other (x TEXT);");
    assert!(read_index(&path).expect("read").threads.is_empty());
    assert!(read_index(&dir.join("absent.sqlite").to_string_lossy()).is_none());
    assert!(
        load(Some(&dir.join("absent").to_string_lossy()))
            .threads
            .is_empty()
    );
    assert!(load(None).threads.is_empty());
}

#[test]
fn current_db_is_the_one_written_last() {
    let dir = temp_dir("pick");
    let dir_s = dir.to_string_lossy().into_owned();
    assert_eq!(current_db(&dir_s), None);
    let old = db(&dir, "state.sqlite", LEGACY_DDL);
    assert_eq!(current_db(&dir_s).as_deref(), Some(old.as_str()));
    let new = db(&dir, "statev2.sqlite", V2_DDL);
    let later = SystemTime::now() + Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .and_then(|f| f.set_modified(later))
        .expect("touch");
    assert_eq!(current_db(&dir_s).as_deref(), Some(old.as_str()));
    std::fs::write(format!("{new}-wal"), b"").expect("wal");
    std::fs::File::options()
        .write(true)
        .open(format!("{new}-wal"))
        .and_then(|f| f.set_modified(later + Duration::from_secs(60)))
        .expect("touch wal");
    assert_eq!(current_db(&dir_s).as_deref(), Some(new.as_str()));
}

#[test]
fn load_rereads_after_the_db_changes() {
    let dir = temp_dir("reload");
    let dir_s = dir.to_string_lossy().into_owned();
    let path = db(
        &dir,
        "statev2.sqlite",
        &format!(
            "{V2_DDL}
            INSERT INTO orchestration_v2_projection_threads VALUES ('t1', 'First', '{{}}', NULL, NULL);
            INSERT INTO orchestration_v2_projection_provider_threads VALUES
              ('p1', 't1', '1', '{{\"nativeThreadRef\":{{\"driver\":\"claudeAgent\",\"nativeId\":\"c-1\"}}}}');"
        ),
    );
    let title = || {
        load(Some(&dir_s))
            .get(Agent::Claude, "c-1")
            .and_then(|t| t.title.clone())
    };
    assert_eq!(title().as_deref(), Some("First"));
    Connection::open(&path)
        .and_then(|c| {
            c.execute(
                "UPDATE orchestration_v2_projection_threads SET title = 'Renamed, longer'",
                [],
            )
        })
        .expect("rename");
    bump(&path);
    assert_eq!(title().as_deref(), Some("Renamed, longer"));
}

#[test]
fn reading_leaves_the_db_unchanged() {
    let dir = temp_dir("ro");
    let path = db(&dir, "statev2.sqlite", V2_DDL);
    let before = std::fs::read(&path).expect("read");
    let _ = read_index(&path);
    assert_eq!(std::fs::read(&path).expect("read"), before);
}

#[test]
fn a_locked_db_keeps_the_last_good_index() {
    let dir = temp_dir("locked");
    let dir_s = dir.to_string_lossy().into_owned();
    let path = db(
        &dir,
        "statev2.sqlite",
        &format!(
            "{V2_DDL}
            INSERT INTO orchestration_v2_projection_threads VALUES ('t1', 'Named', '{{}}', NULL, NULL);
            INSERT INTO orchestration_v2_projection_provider_threads VALUES
              ('p1', 't1', '1', '{{\"nativeThreadRef\":{{\"driver\":\"claudeAgent\",\"nativeId\":\"c-1\"}}}}');"
        ),
    );
    let title = || {
        load(Some(&dir_s))
            .get(Agent::Claude, "c-1")
            .and_then(|t| t.title.clone())
    };
    assert_eq!(title().as_deref(), Some("Named"));
    let writer = Connection::open(&path).expect("open");
    writer
        .execute_batch("BEGIN EXCLUSIVE; UPDATE orchestration_v2_projection_threads SET title = 'Held, not committed';")
        .expect("lock");
    assert!(read_index(&path).is_none());
    bump(&path);
    assert_eq!(title().as_deref(), Some("Named"));
    writer.execute_batch("ROLLBACK;").expect("unlock");
}
