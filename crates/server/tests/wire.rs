//! Every session, memory and stats answer the inspector reads, read into the
//! `peektrace_protocol::wire` types and written back: the value must not
//! change. A key the type lacks, a missing key, or `null` where the type
//! leaves the key out (or the other way round) fails, so the generated TS
//! types describe exactly what the server sends.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::stats_fixture::{self, FIXED_MS, SID};
use common::{Options, TestServer, fixtures, start};
use peektrace_core::AgentRoots;
use peektrace_core::roots::{RootsEnv, Settings, Source};
use peektrace_protocol::wire::{events, memory, sessions, stats};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Numbers as `f64`, so `1` and `1.0` compare equal.
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

/// Path of the first place `raw` and `back` differ.
fn first_diff(raw: &Value, back: &Value, at: &str) -> Option<String> {
    match (raw, back) {
        (Value::Object(was), Value::Object(now)) => {
            was.keys()
                .chain(now.keys())
                .find_map(|key| match (was.get(key), now.get(key)) {
                    (Some(old), Some(new)) => first_diff(old, new, &format!("{at}.{key}")),
                    (old, _) => Some(format!(
                        "{at}.{key}: {}",
                        if old.is_some() { "dropped" } else { "added" }
                    )),
                })
        }
        (Value::Array(was), Value::Array(now)) if was.len() == now.len() => was
            .iter()
            .zip(now)
            .enumerate()
            .find_map(|(i, (old, new))| first_diff(old, new, &format!("{at}[{i}]"))),
        _ => (raw != back).then(|| format!("{at}: {raw} became {back}")),
    }
}

fn round_trip<T: DeserializeOwned + Serialize>(what: &str, raw: &Value) -> T {
    let typed: T = serde_json::from_value(raw.clone())
        .unwrap_or_else(|e| panic!("{what} does not read as its wire type: {e}"));
    let back = serde_json::to_value(&typed).expect("serialize");
    let (raw, back) = (norm(raw), norm(&back));
    if let Some(at) = first_diff(&raw, &back, what) {
        panic!("{what} changed in a round trip: {at}");
    }
    typed
}

fn get_ok(s: &TestServer, path: &str) -> Value {
    let a = s.get(path);
    assert_eq!(a.status, 200, "{path}: {}", a.body);
    a.json()
}

/// Every route of one session, and of its subagents through `parent`.
fn check_session(s: &TestServer, id: &str, parent: Option<&str>) -> usize {
    let q = parent.map_or_else(String::new, |p| format!("?parent={p}"));
    let amp = if q.is_empty() { "?" } else { "&" };
    let base = format!("/api/sessions/{id}");
    round_trip::<sessions::ParsedSession>(
        &format!("{id} parsed"),
        &get_ok(s, &format!("{base}{q}")),
    );
    let analysis: sessions::AnalyzedSession = round_trip(
        &format!("{id} analysis"),
        &get_ok(s, &format!("{base}/analysis{q}")),
    );
    round_trip::<sessions::SessionSummary>(
        &format!("{id} summary"),
        &get_ok(s, &format!("{base}/summary{q}")),
    );
    round_trip::<events::HistoryPage>(
        &format!("{id} history"),
        &get_ok(s, &format!("{base}/history{q}{amp}limit=500")),
    );
    round_trip::<events::HistoryIndex>(
        &format!("{id} index"),
        &get_ok(s, &format!("{base}/history/index{q}")),
    );
    let mut n = 1;
    if parent.is_none() {
        for sub in &analysis.meta.parsed.subagents {
            if sub.unreadable.is_none() {
                n += check_session(s, &sub.id, Some(id));
            }
        }
    }
    n
}

fn check_listing(s: &TestServer) -> usize {
    let listing = get_ok(s, "/api/sessions");
    let headers = listing["headers"].as_array().expect("headers");
    assert_ne!(headers.len(), 0);
    headers
        .iter()
        .map(|h| {
            let h: sessions::SessionHeader = round_trip("header", h);
            check_session(s, &h.id, None)
        })
        .sum()
}

#[test]
fn sessions_of_every_fixture_agent() {
    let s = start(&Options {
        name: "wire-main",
        ..Options::default()
    });
    assert!(
        check_listing(&s) >= 3,
        "the main fixture, its subagent and codex"
    );
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(fixtures("")),
        codex_sessions: Some("/nonexistent-codex".to_owned()),
        pi_sessions: Some(fixtures("")),
        opencode_data: Some("/nonexistent-opencode".to_owned()),
        ..RootsEnv::default()
    };
    let s = start(&Options {
        name: "wire-flat",
        roots: Some(AgentRoots::resolve(&env, &Settings::default())),
        ..Options::default()
    });
    assert!(check_listing(&s) >= 10, "the Claude and Pi fixture files");
}

#[test]
fn stats_report_drills_and_markers() {
    let root = common::temp_dir("wire-stats");
    stats_fixture::build(&root);
    let s = start(&Options {
        name: "wire-stats",
        roots: Some(stats_fixture::roots(&root)),
        now: Some(Arc::new(|| FIXED_MS)),
        ..Options::default()
    });
    let report: stats::StatsReport =
        round_trip("report", &get_ok(&s, "/api/stats/report?days=3650"));
    let keys = report
        .causes
        .iter()
        .map(|r| ("cause", r.key.clone()))
        .chain(report.fails.iter().map(|r| ("fail", r.key.clone())))
        .chain(report.clusters.iter().map(|r| ("cluster", r.key.clone())))
        .chain(report.time.iter().map(|r| ("time", job_key(r.key))));
    let mut drills = 0;
    for (table, key) in keys {
        let key = percent_encode(&key);
        let path = format!("/api/stats/drill/{table}/{key}?days=3650");
        round_trip::<stats::DrillResult>(&format!("{table} drill"), &get_ok(&s, &path));
        drills += 1;
    }
    assert!(drills >= 4, "{drills} drills");
    let markers: stats::SessionMarkers = round_trip(
        "markers",
        &get_ok(&s, &format!("/api/sessions/{SID}/markers")),
    );
    assert_ne!(markers.markers.len(), 0);
}

fn job_key(j: stats::Job) -> String {
    serde_json::to_value(j)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

const MEM_SLUG: &str = "-tmp-mem";

fn memory_root() -> PathBuf {
    let base = common::temp_dir("wire-memory-root");
    let mem = base.join(MEM_SLUG).join("memory");
    std::fs::create_dir_all(&mem).expect("mkdir");
    for f in ["nested.md", "flat.md"] {
        std::fs::copy(Path::new(&fixtures("memory")).join(f), mem.join(f)).expect("copy");
    }
    std::fs::write(
        mem.join("plain.md"),
        "No frontmatter, a [link](nested.md) and [[missing-note]].\n",
    )
    .expect("plain");
    std::fs::write(
        mem.join("MEMORY.md"),
        "- [Nested](nested.md) — the nested one\n- [Gone](gone.md) — dangling\n- [[flat]]\nnot a link\n",
    )
    .expect("index");
    std::fs::create_dir_all(base.join("-tmp-empty")).expect("mkdir empty");
    base
}

#[test]
fn memory_reads_and_writes() {
    let root = memory_root();
    let s = start(&Options {
        name: "wire-memory",
        roots: Some(AgentRoots {
            claude: vec![Source {
                id: "claude".to_owned(),
                label: "default".to_owned(),
                home: root.join("home").to_string_lossy().into_owned(),
                projects_root: root.to_string_lossy().into_owned(),
            }],
            ..AgentRoots::default()
        }),
        ..Options::default()
    });
    let all: memory::AllVaults = round_trip("all vaults", &get_ok(&s, "/api/memory"));
    assert!(all.vaults.iter().any(|v| !v.entries.is_empty()));
    let entries = format!("/api/memory/projects/{MEM_SLUG}/entries");
    let body =
        r#"{"name":"gamma","description":"Third","type":"reference","body":"See [[nested]].\n"}"#;
    let created = s.call("POST", &entries, &[], Some(body));
    assert_eq!(created.status, 200, "{}", created.body);
    round_trip::<memory::MemoryEntry>("created entry", &created.json());
    let patch = r#"{"frontmatter":{"description":"Changed"},"body":"New body.\n"}"#;
    let updated = s.call("PATCH", &format!("{entries}/gamma"), &[], Some(patch));
    assert_eq!(updated.status, 200, "{}", updated.body);
    round_trip::<memory::MemoryEntry>("updated entry", &updated.json());
    let deleted = s.call("DELETE", &format!("{entries}/nested"), &[], None);
    assert_eq!(deleted.status, 200, "{}", deleted.body);
    let d: memory::DeleteResult = round_trip("delete", &deleted.json());
    assert_ne!(d.dangling.len(), 0, "plain.md and gamma link to nested");
}
