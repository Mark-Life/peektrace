//! The background writer against real files in a temp dir.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use peektrace_telemetry::store::{DDL, FILE_NAME};
use peektrace_telemetry::{Kind, Outcome, Row, Store, WideEvent, Writer};

fn temp(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("peektrace-telemetry-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn row(id: &str) -> Row {
    Row::of(&WideEvent {
        id: id.to_owned(),
        trace_id: "0000000000000001".to_owned(),
        ts: now(),
        kind: Kind::Cli,
        name: "cli".to_owned(),
        app_version: "test".to_owned(),
        platform: "p".to_owned(),
        duration_ms: 1.0,
        attributes: "{}".to_owned(),
        spans: Vec::new(),
        outcome: Outcome::Success,
    })
}

#[test]
fn writes_in_the_background() {
    let dir = temp("write");
    let w = Writer::start(dir.join("nested"), now());
    w.send(row("a"));
    w.send(row("b"));
    assert!(w.flush(Duration::from_secs(5)));
    let store = Store::open(&dir.join("nested"), now()).unwrap();
    assert_eq!(store.recent(-1, false).unwrap().len(), 2);
    let sql: Vec<String> = store
        .connection()
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let want: Vec<String> = DDL
        .iter()
        .map(|d| d.replace(" IF NOT EXISTS", ""))
        .collect();
    assert_eq!(sql, want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broken_stores_drop_events_quietly() {
    let dir = temp("broken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(FILE_NAME),
        "not a database, just text that is long enough to read",
    )
    .unwrap();
    let w = Writer::start(dir.clone(), now());
    w.send(row("a"));
    let started = Instant::now();
    assert!(!w.flush(Duration::from_secs(5)));
    assert!(started.elapsed() < Duration::from_secs(1));

    let file = temp("file");
    std::fs::write(&file, "x").unwrap();
    let w = Writer::start(file.clone(), now());
    w.send(row("a"));
    assert!(!w.flush(Duration::from_secs(5)));

    let w = Writer::disabled();
    w.send(row("a"));
    assert!(!w.flush(Duration::from_secs(5)));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_held_lock_never_blocks_past_the_wait() {
    let dir = temp("locked");
    let holder = Store::open(&dir, now()).unwrap();
    holder
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    let w = Writer::start(dir.clone(), now());
    w.send(row("a"));
    let started = Instant::now();
    w.flush(Duration::from_millis(300));
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(holder);
    let _ = std::fs::remove_dir_all(&dir);
}
