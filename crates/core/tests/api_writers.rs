//! The JSON writers the API and the CLI share: compact and pretty output
//! hold the same value, and the shapes carry the TS keys.

mod common;

use common::TempDir;
use peektrace_core::Agent;
use peektrace_core::js::value::JsValue;
use peektrace_core::memory::all_vaults;
use peektrace_core::memory::json::all_vaults_json;
use peektrace_core::roots::Source;
use peektrace_core::search::{SearchHit, SearchResult, SearchSession, result_json};
use peektrace_core::stats::markers::SessionMarker;
use peektrace_core::stats::service::{RefreshSummary, markers_json};

fn same_value(compact: &str, pretty: &str) -> JsValue {
    assert!(!compact.contains('\n'), "{compact}");
    assert!(pretty.contains("\n  "), "{pretty}");
    let a = JsValue::parse(compact).expect("compact json");
    assert_eq!(Some(&a), JsValue::parse(pretty).as_ref());
    a
}

#[test]
fn search_result() {
    let r = SearchResult {
        sessions: vec![SearchSession {
            id: "s".to_owned(),
            agent: Agent::Claude,
            project: "-~-proj".to_owned(),
            title: Some("t".to_owned()),
            updated_at: "2026-06-01T10:00:30.000Z".to_owned(),
            source: None,
            match_count: 1,
            hits: vec![SearchHit {
                line: 1,
                snippet: "x".to_owned(),
                match_start: Some(0),
                match_end: Some(1),
            }],
        }],
        truncated: false,
        scanned_sessions: 17,
        unreadable: 1,
    };
    let v = same_value(&result_json(&r, false), &result_json(&r, true));
    assert_eq!(
        v.get("scannedSessions").and_then(JsValue::as_num),
        Some(17.0)
    );
}

#[test]
fn refresh_summary() {
    let s = RefreshSummary {
        transcripts: 24,
        extracted: 2,
        cached: 21,
        skipped: vec!["/a.jsonl".to_owned()],
        facts: 280,
    };
    let v = same_value(&s.to_json(false), &s.to_json(true));
    assert_eq!(
        v.get("skipped")
            .and_then(JsValue::as_arr)
            .map(<[JsValue]>::len),
        Some(1)
    );
}

#[test]
fn session_markers() {
    let m = [SessionMarker {
        seq: 3.0,
        tool_use_id: None,
        detector: "retry".to_owned(),
        label: "retried".to_owned(),
    }];
    let compact = markers_json("sid-1", &m, false);
    same_value(&compact, &markers_json("sid-1", &m, true));
    assert_eq!(
        compact,
        r#"{"schemaVersion":"peektrace-stats/v2","sid":"sid-1","detectorVersion":2,"markers":[{"seq":3,"toolUseId":null,"detector":"retry","label":"retried"}]}"#
    );
}

#[test]
fn all_vaults_holds_projects_vaults_and_labels() {
    let d = TempDir::new("api-writers-vaults");
    d.write(
        "-tmp-p/memory/a.md",
        "---\nname: a\ndescription: A\ntype: user\n---\nbody\n",
    );
    d.write("-tmp-p/memory/MEMORY.md", "- [A](a.md) — a\n");
    let claude = [Source {
        id: "claude".to_owned(),
        label: "default".to_owned(),
        home: d.join("home"),
        projects_root: d.str(),
    }];
    let all = all_vaults(&claude);
    let v = same_value(&all_vaults_json(&all, false), &all_vaults_json(&all, true));
    let len = |k: &str| v.get(k).and_then(JsValue::as_arr).map(<[JsValue]>::len);
    assert_eq!(
        (len("projects"), len("vaults"), len("labels")),
        (Some(1), Some(1), Some(1))
    );
}
