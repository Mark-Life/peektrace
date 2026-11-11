//! `OpenCode` reader robustness, refs, SQL order and the TS dialect byte
//! count. Ports `opencode-reader.test.ts`, the `OpenCode` cases of
//! `agents-walkers.test.ts`, `agents.test.ts` and `list-cache.test.ts`.

mod common;

use std::fs;

use common::{create_db, headers, ids, opencode_roots, path_str, temp_dir, write_good_db};
use peektrace_core::opencode::{list_session_files, load_session, parse_opencode_session};
use peektrace_core::roots::{RootsEnv, sources};
use peektrace_core::{Agent, ProjectSlug, SessionFileRef, SessionId};
use rusqlite::{Connection, params};

/// Text of the first event of the session `path` names.
fn first_body(data: &str, id: &str) -> String {
    let path = format!("{data}#{id}");
    let p = parse_opencode_session(load_session(&path, id, data).as_ref(), &path, id);
    assert_eq!(p.session_id, id);
    p.events.first().map(|e| e.body.clone()).unwrap_or_default()
}

#[test]
fn corrupt_sibling_db_keeps_the_healthy_one() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_good", "/good/proj");
    fs::write(dir.join("opencode-bad.db"), "not a database at all").expect("write");
    assert!(ids(&list_session_files(&opencode_roots(&data).opencode)).contains(&"ses_good"));
    assert_eq!(first_body(&data, "ses_good"), "hi from ses_good");
    assert!(headers(&data)[0].size_bytes > 0);
}

#[test]
fn zero_byte_db_is_skipped() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_good", "/good/proj");
    fs::write(dir.join("opencode-empty.db"), "").expect("write");
    assert!(ids(&list_session_files(&opencode_roots(&data).opencode)).contains(&"ses_good"));
    assert_eq!(first_body(&data, "ses_good"), "hi from ses_good");
}

#[test]
fn unknown_id_loads_nothing() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_good", "/good/proj");
    assert!(load_session(&format!("{data}#nonexistent-id"), "x", &data).is_none());
}

#[test]
fn session_only_in_the_second_db_resolves() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_a", "/a");
    write_good_db(&dir.join("opencode-2.db"), "ses_b", "/b");
    let refs = list_session_files(&opencode_roots(&data).opencode);
    let got = ids(&refs);
    assert!(got.contains(&"ses_a") && got.contains(&"ses_b"));
    assert_eq!(first_body(&data, "ses_b"), "hi from ses_b");
}

#[test]
fn refs_are_data_dir_hash_id_slugged_by_directory() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_walk", "/demo/project");
    assert_eq!(
        list_session_files(&opencode_roots(&data).opencode),
        [SessionFileRef {
            id: SessionId::new("ses_walk".to_owned()),
            slug: ProjectSlug::new("/demo/project".to_owned()),
            path: format!("{data}#ses_walk"),
            stamp: peektrace_core::FileStamp::default(),
            source: None,
        }]
    );
}

#[test]
fn no_hash_path_loads_from_the_default_root() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_walk", "/demo/project");
    let [r] = list_session_files(&opencode_roots(&data).opencode)
        .try_into()
        .expect("one ref");
    let loaded = load_session(&r.path, r.id.as_str(), "/elsewhere");
    assert!(loaded.is_some());
    assert_eq!(load_session("no-hash", "ses_walk", &data), loaded);
    assert!(load_session(&format!("{data}#ses_missing"), "ses_missing", &data).is_none());
}

#[test]
fn opencode_is_a_readable_agent_with_a_root() {
    assert!(Agent::Opencode.has_reader());
    let env = RootsEnv {
        home: "/home/u".to_owned(),
        ..RootsEnv::default()
    };
    assert_ne!(sources(Agent::Opencode, &env, &[])[0].projects_root, "");
}

#[test]
fn sessions_are_read_on_every_list() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    write_good_db(&dir.join("opencode.db"), "ses_a", "/w/a");
    write_good_db(&dir.join("opencode-2.db"), "ses_b", "/w/b");
    assert_eq!(headers(&data).len(), 2);
    // No list cache: a change between two lists shows on the second.
    Connection::open(dir.join("opencode.db"))
        .expect("open")
        .execute(
            "UPDATE session SET title = 'renamed' WHERE id = 'ses_a'",
            [],
        )
        .expect("update");
    let hs = headers(&data);
    assert_eq!(hs.len(), 2);
    let a = hs.iter().find(|h| h.id.as_str() == "ses_a").expect("ses_a");
    assert_eq!(a.title.as_deref(), Some("renamed"));
}

#[test]
fn messages_and_parts_follow_the_sql_order() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, time_created, time_updated) VALUES ('ses_order', 0, 0)",
        [],
    )
    .expect("session");
    let messages = [
        ("m_a", 1, r#"{"role":"user","time":{"created":3000}}"#),
        ("m_b", 2000, r#"{"role":"user","time":{}}"#),
        ("m_c", 1, r#"{"role":"user","time":{"created":2000}}"#),
        ("m_d", 1, r#"{"role":"user","time":{"created":"500"}}"#),
        ("m_e", 1, r#"{"role":"user","time":{"created":1000}}"#),
        ("m_f", 1, r#"{"role":"user","time":{"created":2000.75}}"#),
        ("m_0", 5, r#"{"role":"user","time":{"created":2000}}"#),
    ];
    for (id, col, body) in messages {
        db.execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES (?, 'ses_order', ?, ?)",
            params![id, col, body],
        )
        .expect("message");
    }
    let parts = [
        ("p_e_b", "m_e", "second"),
        ("p_e_a", "m_e", "first"),
        ("p_orphan", "m_gone", "orphan"),
    ]
    .into_iter()
    .chain(
        ["m_a", "m_b", "m_c", "m_d", "m_f", "m_0"]
            .into_iter()
            .map(|m| (m, m, m)),
    );
    for (id, msg, text) in parts {
        db.execute(
            "INSERT INTO part (id, message_id, session_id, data) VALUES (?, ?, 'ses_order', ?)",
            params![
                format!("{id}-p"),
                msg,
                format!(r#"{{"type":"text","text":"{text}"}}"#)
            ],
        )
        .expect("part");
    }
    drop(db);
    let path = format!("{data}#ses_order");
    let p = parse_opencode_session(load_session(&path, "x", &data).as_ref(), &path, "x");
    let bodies: Vec<_> = p.events.iter().map(|e| e.body.as_str()).collect();
    assert_eq!(
        bodies,
        ["first", "second", "m_0", "m_b", "m_c", "m_f", "m_a", "m_d"]
    );
    // `m_d` has a string `created`, so its ts comes from the column.
    assert_eq!(p.events[7].ts.as_deref(), Some("1970-01-01T00:00:00.001Z"));
    // `time_updated` 0: the last message's ts ends the session.
    assert_eq!(p.ended_at.as_deref(), Some("1970-01-01T00:00:00.001Z"));
}

#[test]
fn size_bytes_follow_json_stringify() {
    let dir = temp_dir("oc");
    let data = path_str(&dir);
    let db = create_db(&dir.join("opencode.db"));
    db.execute(
        "INSERT INTO session (id, time_created, time_updated) VALUES ('s', 1, 2)",
        [],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES ('m', 's', 5, ?)",
        [r#"{"role":"user","text":"é\u2028\t\u0001","n":1e400,"s":"\ud800"}"#],
    )
    .expect("message");
    db.execute(
        "INSERT INTO part (id, message_id, session_id, data) VALUES ('p', 'm', 's', ?)",
        [r#"{"type":"x","z":-0,"big":1e21,"id":"old"}"#],
    )
    .expect("part");
    drop(db);
    let dialect = [
        r#"{"kind":"session","id":"s","timeCreated":1,"timeUpdated":2,"cost":0,"tokensInput":0,"tokensOutput":0,"tokensReasoning":0,"tokensCacheRead":0,"tokensCacheWrite":0}"#,
        "{\"role\":\"user\",\"text\":\"\u{e9}\u{2028}\\t\\u0001\",\"n\":null,\"s\":\"\\ud800\",\"time\":{\"created\":5},\"id\":\"m\",\"sessionID\":\"s\",\"kind\":\"message\"}",
        r#"{"type":"x","z":0,"big":1e+21,"id":"p","sessionID":"s","messageID":"m","kind":"part"}"#,
    ]
    .join("\n");
    let [h] = headers(&data).try_into().expect("one header");
    assert_eq!(h.size_bytes, dialect.len() as u64);
}
