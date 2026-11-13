//! Session pages: every part is a cut of the one analysis JSON.

mod common;

use common::{analyze_default, claude_roots, fixture, read_fixture, session_env};
use peektrace_core::codex::parse_codex_session;
use peektrace_core::sessions::model::AnalyzedSession;
use peektrace_core::sessions::pages::{PAGE_SIZE, SessionPages, hash_hex};
use peektrace_core::sessions::service::{AnalyzeOverrides, Target, analyze_session_with};
use peektrace_core::sessions::write::analyzed_json_with;
use serde_json::{Value, json};

fn claude(dumb_zone: Option<f64>) -> AnalyzedSession {
    let roots = claude_roots(&fixture("sessions/home"), &fixture("sessions/projects"));
    analyze_session_with(
        session_env(&roots),
        Target::Lookup("11111111-1111-4111-8111-111111111111"),
        None,
        true,
        AnalyzeOverrides {
            window: None,
            dumb_zone,
        },
    )
    .expect("analysis")
}

fn codex(name: &str) -> AnalyzedSession {
    let rel = format!("sessions-codex/{name}.jsonl");
    analyze_default(parse_codex_session(
        &read_fixture(&rel),
        &fixture(&rel),
        "x",
    ))
}

fn all() -> Vec<AnalyzedSession> {
    let mut out = vec![claude(None), claude(Some(0.001))];
    out.extend(
        [
            "rollout-budget",
            "rollout-fixture",
            "rollout-context",
            "rollout-fork",
        ]
        .map(codex),
    );
    out
}

fn parse(s: &str) -> Value {
    serde_json::from_str(s).expect("json")
}

fn page_events(p: &SessionPages, limit: usize) -> Vec<Value> {
    (0..p.total())
        .step_by(limit)
        .flat_map(|o| match parse(&p.page(o, limit)) {
            Value::Object(mut m) => match m.remove("events") {
                Some(Value::Array(e)) => e,
                _ => Vec::new(),
            },
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn summary_is_the_analysis_without_its_arrays() {
    for a in all() {
        let full = analyzed_json_with(&a, false);
        let pages = SessionPages::new(a);
        let mut want = parse(&full);
        let obj = want.as_object_mut().expect("object");
        for k in ["events", "turns", "snapshots"] {
            obj.remove(k);
        }
        let mut got = parse(pages.summary());
        let history = got
            .as_object_mut()
            .and_then(|o| o.remove("history"))
            .expect("history");
        assert_eq!(got, want);
        let total = pages.total();
        assert_eq!(history["rev"], json!(hash_hex(&full)));
        assert_eq!(history["total"], json!(total));
        assert_eq!(history["pageSize"], json!(PAGE_SIZE));
        let n = history["pages"].as_array().map_or(0, Vec::len);
        assert_eq!(n, total.div_ceil(PAGE_SIZE));
        assert_eq!(history["index"], parse(pages.index())["hash"], "index hash");
        assert_eq!(history["growth"], parse(pages.growth())["hash"]);
    }
}

#[test]
fn pages_concatenate_to_the_events() {
    for a in all() {
        let full = parse(&analyzed_json_with(&a, false));
        let pages = SessionPages::new(a);
        for limit in [100, 7, 500, 1] {
            assert_eq!(Value::Array(page_events(&pages, limit)), full["events"]);
        }
        let hashes = parse(pages.summary())["history"]["pages"].clone();
        let first = parse(&pages.page(0, PAGE_SIZE));
        assert_eq!(first["hash"], hashes[0]);
        assert_eq!(first["offset"], json!(0));
        let past = parse(&pages.page(pages.total(), 100));
        assert_eq!(past["events"], json!([]));
        assert_eq!(past["hash"], json!(hash_hex("[]")));
    }
}

#[test]
fn index_rows_drop_the_text_and_keep_the_rest() {
    for a in all() {
        let full = parse(&analyzed_json_with(&a, false));
        let pages = SessionPages::new(a);
        let index = parse(pages.index());
        let rows = index["rows"].as_array().cloned().unwrap_or_default();
        let events = full["events"].as_array().cloned().unwrap_or_default();
        assert_eq!(rows.len(), events.len());
        for (pos, (row, e)) in rows.iter().zip(&events).enumerate() {
            let mut want = e.clone();
            let w = want.as_object_mut().expect("event");
            for k in ["body", "preview", "requestId", "ts"] {
                w.remove(k);
            }
            if let Some(Value::Object(edit)) = w.get_mut("contextEdit") {
                edit.remove("original");
            }
            let body = e["body"].as_str().unwrap_or_default();
            w.insert("bodyLen".into(), json!(body.encode_utf16().count()));
            let first = events.iter().position(|x| x["body"] == e["body"]);
            assert_eq!(row["bodyId"], json!(first), "bodyId at {pos}");
            w.insert("bodyId".into(), row["bodyId"].clone());
            w.insert("turn".into(), row["turn"].clone());
            assert_eq!(row, &want, "row {pos}");
        }
        let turns = full["turns"].as_array().cloned().unwrap_or_default();
        let mut cur = 0;
        for (row, e) in rows.iter().zip(&events) {
            if let Some(r) = e["requestId"].as_str().filter(|r| !r.is_empty())
                && let Some(i) = turns.iter().rposition(|t| t["requestId"] == r)
            {
                cur = i + 1;
            }
            assert_eq!(row["turn"], json!(cur));
        }
        let cross = full["dumbZoneCrossTurn"].as_i64().unwrap_or(-1);
        let want = usize::try_from(cross).ok().map_or(-1, |t| {
            let req = turns.get(t).map_or(Value::Null, |t| t["requestId"].clone());
            events
                .iter()
                .position(|e| e.get("requestId").cloned().unwrap_or(Value::Null) == req)
                .map_or(-1, |p| i64::try_from(p).unwrap_or(-1))
        });
        assert_eq!(index["crossEvent"], json!(want));
    }
}

#[test]
fn some_fixture_crosses_the_dumb_zone() {
    let index = parse(SessionPages::new(claude(Some(0.001))).index());
    assert!(index["crossEvent"].as_i64().unwrap_or(-1) >= 0);
}

#[test]
fn growth_is_the_turns_and_snapshots_as_columns() {
    for a in all() {
        let full = parse(&analyzed_json_with(&a, false));
        let pages = SessionPages::new(a);
        let g = parse(pages.growth());
        let snaps = full["snapshots"].as_array().cloned().unwrap_or_default();
        let col = |k: &str| g["snapshots"][k].clone();
        assert_eq!(
            col("turnIndex"),
            Value::Array(snaps.iter().map(|s| s["turnIndex"].clone()).collect())
        );
        assert_eq!(
            col("ctx"),
            Value::Array(snaps.iter().map(|s| s["ctx"].clone()).collect())
        );
        for (k, v) in g["snapshots"]["slices"]
            .as_object()
            .cloned()
            .unwrap_or_default()
        {
            let want: Vec<Value> = snaps.iter().map(|s| s["slices"][&k].clone()).collect();
            assert_eq!(v, Value::Array(want), "slice {k}");
        }
        let turns = full["turns"].as_array().cloned().unwrap_or_default();
        let keep = [
            "model",
            "contextTokens",
            "outputTokens",
            "thinkingTokens",
            "effort",
            "webSearchRequests",
            "webFetchRequests",
        ];
        let want: Vec<Value> = turns
            .iter()
            .map(|t| {
                let o = t.as_object().cloned().unwrap_or_default();
                Value::Object(
                    o.into_iter()
                        .filter(|(k, _)| keep.contains(&k.as_str()))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(g["turns"], Value::Array(want));
        let mut body = g.as_object().cloned().unwrap_or_default();
        body.remove("rev");
        let hash = body.remove("hash");
        assert!(hash.is_some_and(|h| h.as_str().is_some_and(|h| h.len() == 16)));
    }
}

#[test]
fn search_folds_case_like_to_lower_case() {
    let pages = SessionPages::new(claude(None));
    let events = &pages.analysis().parsed.events;
    let expect = |q: &str| -> Vec<usize> {
        let needle = q.to_lowercase();
        events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                format!("{}\n{}\n{}", e.title, e.preview, e.body)
                    .to_lowercase()
                    .contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    };
    let title = events[events.len() / 2].title.to_uppercase();
    for q in ["a", "READ", " ", "\n", "zzz-no-match", title.as_str()] {
        assert_eq!(pages.search(q), expect(q), "{q:?}");
    }
    assert_ne!(pages.search(&title), Vec::<usize>::new());
    assert_eq!(pages.search("zzz-no-match"), Vec::<usize>::new());
    let json = parse(&pages.search_json(&[1, 4]));
    assert_eq!(json["positions"], json!([1, 4]));
    assert_eq!(json["rev"].as_str().map(str::len), Some(16));
}
