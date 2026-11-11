//! `OpenCode` sessions from throwaway SQLite stores and legacy trees, built
//! with the DDL of `packages/core/test/fixtures/opencode-db.ts`. Expected
//! values come from the TS reader and parser tests.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use peektrace_core::js::time::Utc;
use peektrace_core::opencode::{list_session_files, load_session, parse_opencode_session};
use peektrace_core::roots::{RootsEnv, sources};
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::{Agent, AgentRoots, ListFilter, SessionHeader, list_sessions};
use rusqlite::{Connection, params};

const SESSION_DDL: &str = "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, version TEXT, model TEXT, agent TEXT, cost REAL, tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, tokens_cache_write INTEGER, time_created INTEGER, time_updated INTEGER)";
const MESSAGE_DDL: &str = "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";
const PART_DDL: &str = "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";

/// A fresh, empty data dir.
fn temp_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("peektrace-oc-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

fn create_db(path: &std::path::Path) -> Connection {
    let db = Connection::open(path).expect("open");
    for ddl in [SESSION_DDL, MESSAGE_DDL, PART_DDL] {
        db.execute(ddl, []).expect("ddl");
    }
    db
}

/// `writeGoodDb`: one text-only session.
fn write_good_db(path: &std::path::Path, id: &str, directory: &str) {
    let db = create_db(path);
    db.execute(
        "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)",
        params![id, directory, "Good session", 1000, 2000],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)",
        params![
            format!("{id}-m"),
            id,
            1500,
            r#"{"role":"user","time":{"created":1500}}"#
        ],
    )
    .expect("message");
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)",
        params![
            format!("{id}-p"),
            format!("{id}-m"),
            id,
            1501,
            format!(r#"{{"type":"text","text":"hi from {id}"}}"#)
        ],
    )
    .expect("part");
}

fn write_json(path: &std::path::Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
}

fn roots(data: &str) -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        opencode_data: Some(data.to_owned()),
        ..RootsEnv::default()
    };
    AgentRoots {
        opencode: sources(Agent::Opencode, &env, &[]),
        ..AgentRoots::default()
    }
}

fn headers(data: &str) -> Vec<SessionHeader> {
    list_sessions(
        &roots(data),
        &ListFilter {
            agent: Some(Agent::Opencode),
            project: None,
        },
    )
    .headers
}

fn ids(refs: &[peektrace_core::SessionFileRef]) -> Vec<&str> {
    refs.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn db_wins_over_tree_and_tree_only_is_kept() {
    let dir = temp_dir();
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_db1", "/db/proj");
    let st = dir.join("storage");
    write_json(
        &st.join("session/p/ses_db1.json"),
        r#"{"id":"ses_db1","directory":"/tree/copy","time":{"created":1,"updated":2}}"#,
    );
    write_json(
        &st.join("session/p/ses_tree.json"),
        r#"{"id":"ses_tree","directory":"/tree/proj","title":"Tree","time":{"created":100,"updated":200}}"#,
    );
    write_json(
        &st.join("message/ses_tree/msg_a.json"),
        r#"{"role":"user","time":{"created":150}}"#,
    );
    write_json(
        &st.join("part/msg_a/prt_1.json"),
        r#"{"type":"text","text":"hi from tree"}"#,
    );
    let refs = list_session_files(&roots(&data).opencode);
    assert_eq!(ids(&refs), ["ses_db1", "ses_tree"]);
    assert_eq!(refs[0].slug.as_str(), "/db/proj");
    assert_eq!(refs[0].path, format!("{data}#ses_db1"));
    let hs = headers(&data);
    let db1 = hs.iter().find(|h| h.id.as_str() == "ses_db1").expect("db1");
    assert_eq!(db1.cwd.as_deref(), Some("/db/proj"));
    let tree = hs
        .iter()
        .find(|h| h.id.as_str() == "ses_tree")
        .expect("tree");
    assert_eq!(tree.project.as_str(), "proj");
    assert_eq!(tree.updated_at, "1970-01-01T00:00:00.200Z");
    let p = parse_opencode_session(
        load_session(&tree.path, "ses_tree", &data).as_ref(),
        &tree.path,
        "ses_tree",
    );
    assert_eq!(p.events.len(), 1);
    assert_eq!(p.events[0].body, "hi from tree");
}

#[test]
fn json_time_beats_the_column_and_round_trips() {
    let dir = temp_dir();
    let data = path_str(&dir);
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES ('ses_a', '/demo/project', 'T', 1000, 2000)",
        [],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message VALUES ('msg1', 'ses_a', 9999999999, NULL, '{\"role\":\"user\",\"time\":{\"created\":1500}}')",
        [],
    )
    .expect("message");
    db.execute(
        "INSERT INTO part VALUES ('prt1', 'msg1', 'ses_a', 1501, NULL, '{\"type\":\"text\",\"text\":\"the prompt\"}')",
        [],
    )
    .expect("part");
    drop(db);
    let session = load_session(&format!("{data}#ses_a"), "x", "/elsewhere");
    let p = parse_opencode_session(session.as_ref(), "p", "x");
    assert_eq!(p.session_id, "ses_a");
    assert_eq!(p.cwd.as_deref(), Some("/demo/project"));
    assert_eq!(p.events[0].body, "the prompt");
    assert_eq!(p.events[0].ts.as_deref(), Some("1970-01-01T00:00:01.500Z"));
    assert_eq!(p.native_context_window, None);
    let [h] = headers(&data).try_into().expect("one header");
    assert_eq!(h.project.as_str(), "project");
    assert_eq!(h.message_count, 1);
    // The JSONL text the TS reader builds for this session, byte for byte.
    let dialect = [
        r#"{"kind":"session","id":"ses_a","timeCreated":1000,"timeUpdated":2000,"cost":0,"tokensInput":0,"tokensOutput":0,"tokensReasoning":0,"tokensCacheRead":0,"tokensCacheWrite":0,"directory":"/demo/project","title":"T"}"#,
        r#"{"role":"user","time":{"created":1500},"id":"msg1","sessionID":"ses_a","kind":"message"}"#,
        r#"{"type":"text","text":"the prompt","id":"prt1","sessionID":"ses_a","messageID":"msg1","kind":"part"}"#,
    ]
    .join("\n");
    assert_eq!(h.size_bytes, dialect.len() as u64);
}

#[test]
fn broken_stores_are_skipped() {
    let dir = temp_dir();
    let data = path_str(&dir);
    fs::write(dir.join("opencode-bad.db"), "not a database at all").expect("write");
    fs::write(dir.join("opencode-empty.db"), "").expect("write");
    fs::create_dir(dir.join("opencode-dir.db")).expect("mkdir");
    write_good_db(&dir.join("opencode-2.db"), "ses_two", "/w/two");
    write_good_db(&dir.join("other.db"), "ses_other", "/w/other");
    let hs = headers(&data);
    assert_eq!(
        hs.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        ["ses_two"]
    );
    assert!(load_session(&format!("{data}#ses_missing"), "x", &data).is_none());
    let empty = parse_opencode_session(None, "p#ses_missing", "ses_missing");
    assert_eq!(empty.session_id, "ses_missing");
    assert!(empty.events.is_empty() && empty.models.is_empty());
}

#[test]
fn malformed_message_json_drops_the_session_from_ls_only() {
    let dir = temp_dir();
    let data = path_str(&dir);
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, time_created, time_updated) VALUES ('ses_bad', 1, 8000)",
        [],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message VALUES ('m1', 'ses_bad', 1, NULL, '{not json')",
        [],
    )
    .expect("message");
    drop(db);
    assert_eq!(
        ids(&list_session_files(&roots(&data).opencode)),
        ["ses_bad"]
    );
    assert_eq!(headers(&data), Vec::new());
    let a = analyze_session(
        SessionEnv {
            roots: &roots(&data),
            unsearched: &[],
            local: &Utc,
        },
        Target::Lookup("ses_b"),
        None,
        true,
    )
    .expect("resolves by prefix");
    assert_eq!(a.parsed.session_id, "ses_bad");
    assert_eq!(a.context_window, 1_000_000.0);
    assert!(a.context_window_inferred && a.parsed.turns.is_empty());
}

/// One gpt-5.5 call with reasoning: session `cost`, message `cost`.
fn cost_db(dir: &std::path::Path, session_cost: f64, message_costs: &[Option<f64>]) {
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, directory, model, cost, time_created, time_updated) VALUES ('ses_x', '/synthetic/proj', '{\"id\":\"gpt-5.5\",\"providerID\":\"openai\"}', ?, 1, 2)",
        params![session_cost],
    )
    .expect("session");
    for (i, cost) in message_costs.iter().enumerate() {
        let cost = cost.map_or(String::new(), |c| format!(r#","cost":{c}"#));
        let data = format!(
            r#"{{"role":"assistant","modelID":"gpt-5.5"{cost},"tokens":{{"input":1000,"output":100,"reasoning":900,"cache":{{"read":0,"write":0}}}},"time":{{"created":{}}}}}"#,
            i + 10
        );
        db.execute(
            "INSERT INTO message VALUES (?, 'ses_x', 1, NULL, ?)",
            params![format!("m{i}"), data],
        )
        .expect("message");
    }
}

/// gpt-5.5: 1000 input at $5 plus (100 output + 900 reasoning) at $30.
const EST: f64 = (1000.0 * 5.0 + 1000.0 * 30.0) / 1e6;

fn cost_of(session_cost: f64, message_costs: &[Option<f64>]) -> (Option<f64>, bool, f64) {
    let dir = temp_dir();
    let data = path_str(&dir);
    cost_db(&dir, session_cost, message_costs);
    let a = analyze_session(
        SessionEnv {
            roots: &roots(&data),
            unsearched: &[],
            local: &Utc,
        },
        Target::Lookup("ses_x"),
        None,
        false,
    )
    .expect("analyze");
    let cost = a.cost.expect("cost");
    let [h] = headers(&data).try_into().expect("one header");
    (
        cost.total_usd,
        cost.from_agent,
        h.cost_usd.unwrap_or(f64::NAN),
    )
}

#[test]
fn costs_follow_the_agent_figure_else_estimate() {
    assert_eq!(cost_of(0.01, &[Some(0.0)]), (Some(0.01), true, 0.01));
    let (total, agent, header) = cost_of(0.0, &[Some(0.0)]);
    assert!(!agent);
    assert!((total.unwrap_or_default() - EST).abs() < 1e-12);
    assert!((header - EST).abs() < 1e-12);
    assert_eq!(cost_of(0.0, &[Some(0.5)]), (Some(0.5), true, 0.5));
    let (total, agent, header) = cost_of(0.0, &[Some(0.5), None]);
    assert!(!agent);
    assert!((total.unwrap_or_default() - (0.5 + EST)).abs() < 1e-12);
    assert!((header - (0.5 + EST)).abs() < 1e-12);
}

#[test]
fn wal_store_reads_the_last_commit_while_a_writer_is_open() {
    let dir = temp_dir();
    let data = path_str(&dir);
    let db = create_db(&dir.join("opencode.db"));
    db.pragma_update(None, "journal_mode", "WAL").expect("wal");
    db.pragma_update(None, "wal_autocheckpoint", 0)
        .expect("no checkpoint");
    db.execute(
        "INSERT INTO session (id, time_created, time_updated) VALUES ('ses_w', 1, 2)",
        [],
    )
    .expect("session");
    db.execute_batch(
        "BEGIN IMMEDIATE; INSERT INTO session (id, time_created, time_updated) VALUES ('ses_pending', 1, 3);",
    )
    .expect("open write");
    let hs = headers(&data);
    assert_eq!(
        hs.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        ["ses_w"]
    );
    db.execute_batch("COMMIT").expect("commit");
}

#[test]
fn locked_rollback_store_is_read_once_the_lock_goes() {
    let dir = temp_dir();
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_l", "/w/l");
    let db = Connection::open(dir.join("opencode.db")).expect("open");
    db.execute_batch("BEGIN EXCLUSIVE; UPDATE session SET title = 'held' WHERE id = 'ses_l';")
        .expect("lock");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        db.execute_batch("COMMIT").expect("commit");
    });
    let hs = headers(&data);
    release.join().expect("join");
    assert_eq!(hs.len(), 1);
    assert_eq!(hs[0].title.as_deref(), Some("held"));
}

#[test]
fn project_filter_uses_the_cwd_basename() {
    let dir = temp_dir();
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_p", "/w/proj");
    let list = |p: &str| {
        list_sessions(
            &roots(&data),
            &ListFilter {
                agent: None,
                project: Some(p.to_owned()),
            },
        )
        .headers
        .len()
    };
    assert_eq!((list("proj"), list("w"), list("")), (1, 0, 1));
}
