//! Paged session routes: summary, growth, history index, event pages and
//! search, each a cut of `/analysis`.

mod common;

use common::{Options, SESSION, TestServer, start, temp_dir};
use serde_json::{Value, json};

fn server() -> TestServer {
    start(&Options {
        name: "history",
        ..Options::default()
    })
}

fn get(s: &TestServer, path: &str) -> Value {
    let a = s.get(path);
    assert_eq!(a.status, 200, "{path}: {}", a.body);
    a.json()
}

fn without(mut v: Value, keys: &[&str]) -> Value {
    if let Some(o) = v.as_object_mut() {
        for k in keys {
            o.remove(*k);
        }
    }
    v
}

fn ids(s: &TestServer) -> Vec<String> {
    get(s, "/api/sessions")["headers"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|h| h["id"].as_str().map(str::to_owned))
        .collect()
}

fn pages_of(s: &TestServer, id: &str, limit: usize, extra: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let p = get(
            s,
            &format!("/api/sessions/{id}/history?offset={offset}&limit={limit}{extra}"),
        );
        let events = p["events"].as_array().cloned().unwrap_or_default();
        if events.is_empty() {
            return out;
        }
        assert_eq!(p["offset"], json!(offset));
        out.extend(events);
        offset += limit;
    }
}

#[test]
fn summary_is_the_analysis_without_its_arrays() {
    let s = server();
    for id in ids(&s) {
        let full = get(&s, &format!("/api/sessions/{id}/analysis"));
        let sum = get(&s, &format!("/api/sessions/{id}/summary"));
        let history = sum["history"].clone();
        assert_eq!(
            without(sum, &["history"]),
            without(full.clone(), &["events", "turns", "snapshots"]),
            "{id}"
        );
        let total = full["events"].as_array().map_or(0, Vec::len);
        assert_eq!(history["total"], json!(total));
        assert_eq!(history["pageSize"], json!(100));
        assert_eq!(
            history["pages"].as_array().map_or(0, Vec::len),
            total.div_ceil(100)
        );
    }
}

#[test]
fn pages_concatenate_to_the_analysis_events() {
    let s = server();
    for id in ids(&s) {
        let full = get(&s, &format!("/api/sessions/{id}/analysis"));
        for limit in [100, 7, 500] {
            assert_eq!(
                Value::Array(pages_of(&s, &id, limit, "")),
                full["events"],
                "{id} limit {limit}"
            );
        }
        let sum = get(&s, &format!("/api/sessions/{id}/summary"));
        let page = get(&s, &format!("/api/sessions/{id}/history"));
        assert_eq!(page["hash"], sum["history"]["pages"][0]);
        assert_eq!(page["rev"], sum["history"]["rev"]);
        let total = sum["history"]["total"].as_u64().unwrap_or(0);
        let past = get(&s, &format!("/api/sessions/{id}/history?offset={total}"));
        assert_eq!(past["events"], json!([]));
        assert_eq!(past["total"], json!(total));
    }
}

#[test]
fn bad_bounds_are_400_and_unknown_ids_404() {
    let s = server();
    for q in [
        "offset=-1",
        "offset=1.5",
        "offset=abc",
        "limit=0",
        "limit=501",
        "limit=",
    ] {
        let a = s.get(&format!("/api/sessions/{SESSION}/history?{q}"));
        assert_eq!(a.status, 400, "{q}");
        assert_eq!(a.json()["error"]["_tag"], "BadRequestError");
    }
    for route in [
        "summary",
        "growth",
        "history",
        "history/index",
        "history/search?q=a",
    ] {
        let a = s.get(&format!("/api/sessions/nope/{route}"));
        assert_eq!(a.status, 404, "{route}");
        assert_eq!(a.json()["error"]["_tag"], "SessionNotFoundError");
    }
    for q in ["", "?q=", "?redact=maybe&q=a"] {
        let a = s.get(&format!("/api/sessions/{SESSION}/history/search{q}"));
        assert_eq!(a.status, 400, "{q:?}");
    }
}

#[test]
fn index_rows_carry_no_text() {
    let s = server();
    for id in ids(&s) {
        let index = get(&s, &format!("/api/sessions/{id}/history/index"));
        let full = get(&s, &format!("/api/sessions/{id}/analysis"));
        let rows = index["rows"].as_array().cloned().unwrap_or_default();
        let events = full["events"].as_array().cloned().unwrap_or_default();
        assert_eq!(rows.len(), events.len());
        for (row, e) in rows.iter().zip(&events) {
            for k in ["body", "preview", "requestId", "ts"] {
                assert!(row.get(k).is_none(), "{id} {k}");
            }
            assert!(row["contextEdit"].get("original").is_none());
            assert_eq!(row["kind"], e["kind"]);
            assert_eq!(row["title"], e["title"]);
            let same = |a: &Value, b: &Value| a["bodyId"] == b["bodyId"];
            for (other, oe) in rows.iter().zip(&events) {
                assert_eq!(same(row, other), e["body"] == oe["body"]);
            }
        }
    }
}

#[test]
fn growth_columns_match_the_snapshots() {
    let s = server();
    for id in ids(&s) {
        let g = get(&s, &format!("/api/sessions/{id}/growth"));
        let full = get(&s, &format!("/api/sessions/{id}/analysis"));
        let snaps = full["snapshots"].as_array().cloned().unwrap_or_default();
        let ctx: Vec<Value> = snaps.iter().map(|x| x["ctx"].clone()).collect();
        assert_eq!(g["snapshots"]["ctx"], Value::Array(ctx));
        let files: Vec<Value> = snaps.iter().map(|x| x["slices"]["files"].clone()).collect();
        assert_eq!(g["snapshots"]["slices"]["files"], Value::Array(files));
        assert_eq!(
            g["turns"].as_array().map(Vec::len),
            full["turns"].as_array().map(Vec::len)
        );
    }
}

fn search(s: &TestServer, id: &str, q: &str, extra: &str) -> Vec<u64> {
    let enc: String = url_encode(q);
    get(
        s,
        &format!("/api/sessions/{id}/history/search?q={enc}{extra}"),
    )["positions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_u64)
        .collect()
}

fn url_encode(q: &str) -> String {
    q.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[test]
fn search_folds_case_and_follows_redaction() {
    let s = server();
    let lower = search(&s, SESSION, "refactor the auth", "");
    assert_ne!(lower, Vec::<u64>::new());
    assert_eq!(search(&s, SESSION, "REFACTOR THE AUTH", ""), lower);
    assert_ne!(search(&s, SESSION, " ", ""), Vec::<u64>::new());
    let secret = "sk-ant-api03-AAAABBBB";
    assert_eq!(search(&s, SESSION, secret, ""), Vec::<u64>::new());
    assert_ne!(
        search(&s, SESSION, secret, "&redact=false"),
        Vec::<u64>::new()
    );
    for id in ids(&s) {
        let full = get(&s, &format!("/api/sessions/{id}/analysis"));
        let events = full["events"].as_array().cloned().unwrap_or_default();
        if let Some(pos) = events.iter().position(|e| e["kind"] == "system") {
            let title = events[pos]["title"].as_str().unwrap_or_default();
            assert!(search(&s, &id, title, "").contains(&(pos as u64)), "{id}");
        }
    }
}

#[test]
fn every_route_takes_a_parent() {
    let s = server();
    let q = format!("?parent={SESSION}");
    let full = get(&s, &format!("/api/sessions/agent-1/analysis{q}"));
    let sum = get(&s, &format!("/api/sessions/agent-1/summary{q}"));
    assert_eq!(sum["sessionId"], full["sessionId"]);
    let page = get(&s, &format!("/api/sessions/agent-1/history{q}"));
    assert_eq!(page["events"], full["events"]);
    let index = get(&s, &format!("/api/sessions/agent-1/history/index{q}"));
    assert_eq!(index["total"], sum["history"]["total"]);
    get(&s, &format!("/api/sessions/agent-1/growth{q}"));
    get(&s, &format!("/api/sessions/agent-1/history/search{q}&q=a"));
    assert_eq!(s.get("/api/sessions/agent-1/summary").status, 404);
}

#[test]
fn odd_limits_page_every_agent() {
    let s = server();
    let agents: Vec<Value> = get(&s, "/api/sessions")["headers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(agents.iter().any(|h| h["agent"] == "codex"));
    for h in agents {
        let id = h["id"].as_str().unwrap_or_default();
        let full = get(&s, &format!("/api/sessions/{id}/analysis"));
        assert_eq!(Value::Array(pages_of(&s, id, 3, "")), full["events"]);
    }
}

fn prompt(i: usize) -> String {
    format!(
        r#"{{"type":"user","cwd":"/w","timestamp":"2026-06-01T10:{:02}:{:02}.000Z","message":{{"content":"prompt number {i}"}}}}"#,
        i / 60,
        i % 60
    )
}

#[test]
fn an_append_moves_the_rev_and_only_the_last_page() {
    let root = temp_dir("history-append");
    let projects = root.join("projects");
    let dir = projects.join("-w");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let id = "22222222-2222-4222-8222-222222222222";
    let file = dir.join(format!("{id}.jsonl"));
    let lines: Vec<String> = (0..150).map(prompt).collect();
    std::fs::write(&file, lines.join("\n") + "\n").expect("write");
    let env = peektrace_core::roots::RootsEnv {
        home: root.to_string_lossy().into_owned(),
        claude_projects: Some(projects.to_string_lossy().into_owned()),
        codex_sessions: Some("/nonexistent-codex".to_owned()),
        pi_sessions: Some("/nonexistent-pi".to_owned()),
        opencode_data: Some("/nonexistent-opencode".to_owned()),
        ..peektrace_core::roots::RootsEnv::default()
    };
    let roots =
        peektrace_core::AgentRoots::resolve(&env, &peektrace_core::roots::Settings::default());
    let s = start(&Options {
        name: "history-append",
        roots: Some(roots),
        ..Options::default()
    });
    let before = get(&s, &format!("/api/sessions/{id}/summary"))["history"].clone();
    assert_eq!(before["total"], json!(150));
    assert_eq!(
        get(&s, &format!("/api/sessions/{id}/summary"))["history"],
        before
    );
    let more = format!("{}\n", prompt(150));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .and_then(|mut f| std::io::Write::write_all(&mut f, more.as_bytes()))
        .expect("append");
    let after = get(&s, &format!("/api/sessions/{id}/summary"))["history"].clone();
    assert_eq!(after["total"], json!(151));
    assert_ne!(after["rev"], before["rev"]);
    assert_eq!(after["pages"][0], before["pages"][0]);
    assert_ne!(after["pages"][1], before["pages"][1]);
    assert_ne!(after["index"], before["index"]);
}

#[test]
fn every_part_reads_into_the_protocol_types() {
    use peektrace_protocol::view::{AnalyzedSession, HistoryIndex, HistoryPage, SessionSummary};
    use peektrace_protocol::{GrowthSeries, HistorySearchResult};
    /// Every key `typed` writes is on the wire too: the types read only
    /// some fields, but never invent one.
    fn within(typed: &Value, wire: &Value, at: &str) {
        match (typed, wire) {
            (Value::Object(t), Value::Object(w)) => {
                for (k, v) in t {
                    let w = w
                        .get(k)
                        .unwrap_or_else(|| panic!("{at}.{k} is not on the wire"));
                    within(v, w, &format!("{at}.{k}"));
                }
            }
            (Value::Array(t), Value::Array(w)) => {
                assert_eq!(t.len(), w.len(), "{at}");
                for (i, (t, w)) in t.iter().zip(w).enumerate() {
                    within(t, w, &format!("{at}[{i}]"));
                }
            }
            _ => {}
        }
    }
    fn round<T>(body: &str) -> T
    where
        T: serde::de::DeserializeOwned + serde::Serialize + PartialEq + std::fmt::Debug,
    {
        let t: T = serde_json::from_str(body).expect("typed");
        let back = serde_json::to_value(&t).expect("value");
        within(&back, &serde_json::from_str(body).expect("wire"), "");
        assert_eq!(serde_json::from_value::<T>(back).ok().as_ref(), Some(&t));
        t
    }
    let s = server();
    for id in ids(&s) {
        let body = |route: &str| {
            let a = s.get(&format!("/api/sessions/{id}/{route}"));
            assert_eq!(a.status, 200);
            a.body
        };
        let growth: GrowthSeries = round(&body("growth"));
        let found: HistorySearchResult = round(&body("history/search?q=a"));
        let sum: SessionSummary = round(&body("summary"));
        let page: HistoryPage = round(&body("history"));
        let index: HistoryIndex = round(&body("history/index"));
        assert_eq!(growth.rev, sum.history.rev);
        assert_eq!(found.rev, sum.history.rev);
        assert_eq!(growth.hash, sum.history.growth);
        assert_eq!(Some(&page.meta.hash), sum.history.pages.first());
        assert_eq!(index.meta.hash, sum.history.index);
        assert_eq!(index.rows.len() as u64, page.meta.total);
        // A full analysis never reads as a summary with no events.
        assert!(serde_json::from_str::<AnalyzedSession>(&body("summary")).is_err());
        let full: AnalyzedSession = round(&body("analysis"));
        assert_eq!(full.meta, sum.meta);
        assert_eq!(full.events.len(), index.rows.len());
    }
}
