//! t3code thread names reach listed and parsed Claude sessions, and survive
//! the JSON round trip remote clients use.

mod common;

use common::TempDir;
use peektrace_core::js::value::JsValue;
use peektrace_core::sessions::read::{listing_from_json, parsed_from_json};
use peektrace_core::sessions::service::{Target, parse_full};
use peektrace_core::sessions::write::{listing_json, parsed_json};
use peektrace_core::{AgentRoots, Launcher, ListFilter, list_sessions};
use rusqlite::Connection;

fn transcript(id: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"sessionId\":\"{id}\",\"cwd\":\"/w\",\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n"
    )
}

/// Three Claude sessions: one from a live thread, one from a deleted thread,
/// one t3code never saw.
fn setup(dir: &TempDir) -> AgentRoots {
    for id in ["s-live", "s-gone", "s-own"] {
        dir.write(&format!("projects/-w/{id}.jsonl"), &transcript(id));
    }
    let state = dir.mkdir("t3/userdata");
    Connection::open(dir.join("t3/userdata/statev2.sqlite"))
        .and_then(|c| {
            c.execute_batch(
                r#"
CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY, title TEXT,
    payload_json TEXT, archived_at TEXT, deleted_at TEXT);
CREATE TABLE orchestration_v2_projection_provider_threads (provider_thread_id TEXT PRIMARY KEY,
    thread_id TEXT, updated_at TEXT, payload_json TEXT);
INSERT INTO orchestration_v2_projection_threads VALUES
  ('t1', 'Thread name', '{"branch":"feat/a","worktreePath":"/wt/a"}', NULL, NULL),
  ('t2', 'Deleted name', '{}', NULL, '2026-01-02');
INSERT INTO orchestration_v2_projection_provider_threads VALUES
  ('p1', 't1', '1', '{"nativeThreadRef":{"driver":"claudeAgent","nativeId":"s-live"}}'),
  ('p2', 't2', '1', '{"nativeThreadRef":{"driver":"claudeAgent","nativeId":"s-gone"}}');
"#,
            )
        })
        .expect("fixture db");
    AgentRoots {
        t3code: Some(state),
        ..common::claude_roots(&dir.str(), &dir.join("projects"))
    }
}

fn launcher(branch: Option<&str>, worktree: Option<&str>) -> Launcher {
    Launcher {
        name: "t3code".into(),
        branch: branch.map(Into::into),
        worktree_path: worktree.map(Into::into),
    }
}

#[test]
fn listed_sessions_take_the_thread_name_and_launcher() {
    let dir = TempDir::new("t3code-list");
    let roots = setup(&dir);
    let listing = list_sessions(&roots, &ListFilter::default());
    let row = |id: &str| {
        listing
            .headers
            .iter()
            .find(|h| h.id.as_str() == id)
            .map(|h| (h.title.clone(), h.launcher.clone()))
            .expect("listed")
    };
    assert_eq!(
        row("s-live"),
        (
            Some("Thread name".into()),
            Some(launcher(Some("feat/a"), Some("/wt/a")))
        )
    );
    assert_eq!(row("s-gone"), (None, Some(launcher(None, None))));
    assert_eq!(row("s-own"), (None, None));

    let json = JsValue::parse(&listing_json(&listing)).expect("json");
    let (back, _) = listing_from_json(&json).expect("listing");
    assert_eq!(back, listing.headers);
}

#[test]
fn parsed_session_takes_the_thread_name_and_launcher() {
    let dir = TempDir::new("t3code-parse");
    let roots = setup(&dir);
    let parsed = parse_full(common::session_env(&roots), Target::Lookup("s-live"), None)
        .expect("parsed")
        .parsed;
    assert_eq!(parsed.title.as_deref(), Some("Thread name"));
    assert_eq!(
        parsed.launcher,
        Some(launcher(Some("feat/a"), Some("/wt/a")))
    );

    let json = JsValue::parse(&parsed_json(&parsed, false)).expect("json");
    assert_eq!(
        parsed_from_json(&json).map(|p| p.launcher),
        Some(parsed.launcher)
    );
}

#[test]
fn no_t3code_dir_leaves_sessions_alone() {
    let dir = TempDir::new("t3code-none");
    let roots = AgentRoots {
        t3code: None,
        ..setup(&dir)
    };
    let listing = list_sessions(&roots, &ListFilter::default());
    assert!(listing.headers.iter().all(|h| h.launcher.is_none()));
}
