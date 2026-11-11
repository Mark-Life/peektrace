//! The fact cache: index merging, the window skip, shard pruning and reads,
//! and usage facts through a shard round trip.
#![allow(clippy::float_cmp, reason = "epoch ms and row counts are exact")]

mod common;

use std::collections::HashSet;

use common::{TempDir, bash_fact, session_fact};
use indexmap::IndexMap;
use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::{Utc, parse_date};
use peektrace_core::stats::cache::{
    CacheEntry, FileStat, StatsCache, fact_ms, is_past_window, last_fact_ms_of, merge_index,
};
use peektrace_core::stats::schema::{EXTRACTOR_VERSION, Fact, fact_json};

fn entry(path: &str, rows: f64) -> CacheEntry {
    CacheEntry {
        path: path.to_owned(),
        mtime_ms: 1.0,
        size: 1.0,
        shard: format!("{}.jsonl", path.replace('/', "_")),
        rows,
        last_fact_ms: None,
        extractor_version: EXTRACTOR_VERSION,
    }
}

fn prev() -> IndexMap<String, CacheEntry> {
    ["/a/1.jsonl", "/a/2.jsonl", "/b/1.jsonl"]
        .into_iter()
        .map(|p| (p.to_owned(), entry(p, 1.0)))
        .collect()
}

fn by_path(entries: &[CacheEntry]) -> Vec<(&str, f64)> {
    let mut v: Vec<(&str, f64)> = entries.iter().map(|e| (e.path.as_str(), e.rows)).collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    v
}

#[test]
fn a_scoped_pass_keeps_paths_outside_its_scope() {
    let next = merge_index(
        &prev(),
        &[entry("/b/1.jsonl", 7.0), entry("/b/2.jsonl", 2.0)],
        &["/b/1.jsonl".into(), "/b/2.jsonl".into()],
    );
    assert_eq!(
        by_path(&next),
        [
            ("/a/1.jsonl", 1.0),
            ("/a/2.jsonl", 1.0),
            ("/b/1.jsonl", 7.0),
            ("/b/2.jsonl", 2.0)
        ]
    );
}

#[test]
fn a_scanned_path_with_no_new_entry_is_dropped() {
    let next = merge_index(&prev(), &[], &["/b/1.jsonl".into()]);
    assert_eq!(by_path(&next), [("/a/1.jsonl", 1.0), ("/a/2.jsonl", 1.0)]);
}

const T1: &str = "2026-08-01T00:00:00.000Z";
const T2: &str = "2026-08-03T00:00:00.000Z";

fn at(iso: &str) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "epoch ms fit in f64")]
    let v = parse_date(iso, &Utc).expect("date") as f64;
    v
}

fn bash_at(ts: Option<&str>) -> Fact {
    let mut f = bash_fact("true");
    f.ts = ts.map(str::to_owned);
    Fact::Call(f)
}

fn session_at(started: Option<&str>) -> Fact {
    let mut s = session_fact();
    s.started_at = started.map(str::to_owned);
    Fact::Session(s)
}

#[test]
fn fact_ms_reads_ts_or_started_at() {
    assert_eq!(fact_ms(&bash_at(Some(T1)), &Utc), Some(at(T1)));
    assert_eq!(fact_ms(&session_at(Some(T2)), &Utc), Some(at(T2)));
}

#[test]
fn an_undated_or_unparsable_fact_has_no_time() {
    assert_eq!(fact_ms(&bash_at(None), &Utc), None);
    assert_eq!(fact_ms(&bash_at(Some("not a date")), &Utc), None);
    assert_eq!(fact_ms(&session_at(None), &Utc), None);
}

#[test]
fn last_fact_ms_is_the_newest_and_none_once_undated() {
    let mut dated = vec![session_at(Some(T1)), bash_at(Some(T2))];
    assert_eq!(last_fact_ms_of(&dated, &Utc), Some(at(T2)));
    dated.push(bash_at(Some("bad")));
    assert_eq!(last_fact_ms_of(&dated, &Utc), None);
    assert_eq!(last_fact_ms_of(&[], &Utc), Some(0.0));
}

#[test]
fn only_a_shard_wholly_before_the_window_is_skipped() {
    let with_last = |l: Option<f64>| CacheEntry {
        last_fact_ms: l,
        ..entry("/a/1.jsonl", 1.0)
    };
    assert!(is_past_window(&with_last(Some(at(T1))), at(T2)));
    assert!(!is_past_window(&with_last(Some(at(T2))), at(T2)));
    assert!(!is_past_window(&with_last(Some(at(T2))), at(T1)));
    assert!(!is_past_window(&with_last(None), at(T2)));
    assert!(is_past_window(&with_last(Some(0.0)), at(T1)));
}

#[test]
fn prune_deletes_unreferenced_shards_and_leaves_other_files() {
    let t = TempDir::new("cache");
    for name in ["keep.jsonl", "dead.jsonl", "notes.txt"] {
        t.write(&format!("stats-rs/shards/{name}"), "");
    }
    StatsCache::new(t.path("stats-rs")).prune(&HashSet::from(["keep.jsonl".to_owned()]));
    let mut left: Vec<String> = std::fs::read_dir(t.path("stats-rs/shards"))
        .expect("shards")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    left.sort();
    assert_eq!(left, ["keep.jsonl", "notes.txt"]);
}

#[test]
fn prune_succeeds_when_no_shard_dir_exists() {
    let t = TempDir::new("cache");
    StatsCache::new(t.path("stats-rs")).prune(&HashSet::new());
    assert!(!std::path::Path::new(&t.path("stats-rs/shards")).exists());
}

fn read_body(body: &str) -> Option<Vec<Fact>> {
    let t = TempDir::new("cache");
    let target = entry("s.jsonl", 1.0);
    t.write(&format!("stats-rs/shards/{}", target.shard), body);
    StatsCache::new(t.path("stats-rs")).read(&target)
}

#[test]
fn a_shard_that_is_not_json_fails_cleanly() {
    assert_eq!(read_body("not json"), None);
}

#[test]
fn a_shard_cut_mid_line_fails_cleanly() {
    let line = fact_json(&bash_at(Some(T1)));
    let body = format!("{line}\n{}", &line[..line.len() / 2]);
    assert_eq!(read_body(&body), None);
}

const RETRIES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/core/test/fixtures/sessions-claude/retries.jsonl"
);

#[test]
fn one_usage_fact_per_parsed_turn() {
    let text = std::fs::read_to_string(RETRIES).expect("fixture");
    let parsed = parse_claude_session(&ParseArgs {
        text: &text,
        path: RETRIES,
        session_id: "retries",
        include_sidechain_turns: false,
        skip_system_prompt: false,
    });
    let facts = common::facts_of_text(&text, "retries", true, false);
    let reqs: Vec<&str> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::Usage(u) => Some(u.req.as_str()),
            _ => None,
        })
        .collect();
    assert!(!parsed.turns.is_empty());
    let want: Vec<&str> = parsed.turns.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(reqs, want);
}

#[test]
fn only_tool_and_bash_rows_are_call_facts() {
    let text = std::fs::read_to_string(RETRIES).expect("fixture");
    let facts = common::facts_of_text(&text, "retries", true, false);
    let usage: Vec<&Fact> = facts
        .iter()
        .filter(|f| matches!(f, Fact::Usage(_)))
        .collect();
    assert!(!usage.is_empty());
    assert!(usage.iter().all(|f| f.as_call().is_none()));
}

#[test]
fn usage_facts_survive_a_cache_round_trip() {
    let text = std::fs::read_to_string(RETRIES).expect("fixture");
    let facts = common::facts_of_text(&text, "retries", true, false);
    let t = TempDir::new("cache");
    let cache = StatsCache::new(t.path("stats-rs"));
    let stat = FileStat {
        size: 1.0,
        mtime_ms: 1.0,
    };
    let e = cache
        .write("/fixture/proj/retries.jsonl", stat, &facts, &Utc)
        .expect("entry");
    let back = cache.read(&e).expect("shard");
    let usage = |v: &[Fact]| -> Vec<Fact> {
        v.iter()
            .filter(|f| matches!(f, Fact::Usage(_)))
            .cloned()
            .collect()
    };
    assert_eq!(usage(&back), usage(&facts));
}
