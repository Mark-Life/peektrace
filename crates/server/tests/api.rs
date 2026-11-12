//! `/api` routes on the core fixtures (`packages/rpc/test/rpc.test.ts`).

mod common;

use common::{Options, SESSION, start};
use peektrace_core::js::time::Utc;
use peektrace_core::sessions::service::{SessionEnv, Target, analyze_session};
use peektrace_core::sessions::write::analyzed_json_with;

fn tag(a: &common::Answer) -> String {
    a.json()["error"]["_tag"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn capabilities_and_server_info() {
    let s = start(&Options {
        name: "info",
        ..Options::default()
    });
    let caps = s.get("/api/capabilities").json();
    assert_eq!(caps.as_array().map(Vec::len), Some(7));
    assert_eq!(caps[0]["id"], "session.view");
    assert_eq!(caps[1]["perAgent"]["codex"]["level"], "partial");
    let info = s.get("/api/server").json();
    assert_eq!(info["readOnly"], false);
    assert_eq!(info["userHome"], "/home/test");
    let agents: Vec<_> = info["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .map(|a| a["agent"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(agents, ["claude", "codex", "pi", "opencode"]);
    assert_eq!(info["agents"][0]["roots"][0]["exists"], true);
    assert_eq!(info["agents"][0]["roots"][0]["isDefault"], true);
    assert_eq!(info["agents"][2]["roots"][0]["exists"], false);
}

#[test]
fn listing_filters() {
    let s = start(&Options {
        name: "listing",
        ..Options::default()
    });
    let all = s.get("/api/sessions").json();
    let rows = all["headers"].as_array().expect("headers");
    assert!(rows.iter().any(|h| h["agent"] == "claude"));
    assert!(rows.iter().any(|h| h["agent"] == "codex"));
    assert!(all["unreadable"].is_array());
    let codex = s.get("/api/sessions?agent=codex").json();
    assert!(
        codex["headers"]
            .as_array()
            .expect("rows")
            .iter()
            .all(|h| h["agent"] == "codex")
    );
    let proj = s.get("/api/sessions?project=-Users-demo-proj").json();
    let proj = proj["headers"].as_array().expect("rows");
    assert_ne!(proj.len(), 0);
    assert!(proj.iter().all(|h| h["project"] == "-Users-demo-proj"));
    let none = s.get("/api/sessions?project=no-such").json();
    assert_eq!(none["headers"].as_array().map(Vec::len), Some(0));
    let bad = s.get("/api/sessions?agent=all");
    assert_eq!(bad.status, 400);
    assert_eq!(tag(&bad), "BadRequestError");
}

#[test]
fn get_folds_subagents_and_analysis_matches_core() {
    let s = start(&Options {
        name: "get",
        ..Options::default()
    });
    let got = s.get(&format!("/api/sessions/{SESSION}")).json();
    assert_eq!(got["sessionId"], SESSION);
    assert_eq!(got["subagents"][0]["id"], "agent-1");
    let nested = s.get(&format!("/api/sessions/agent-1?parent={SESSION}"));
    assert_eq!(nested.status, 200);
    let body = s.get(&format!("/api/sessions/{SESSION}/analysis")).body;
    let roots = common::fixture_roots();
    let env = SessionEnv {
        roots: &roots,
        local: &Utc,
    };
    let want = analyze_session(env, Target::Lookup(SESSION), None, true).expect("analyze");
    assert_eq!(body, analyzed_json_with(&want, false));
    let windowed = s
        .get(&format!("/api/sessions/{SESSION}/analysis?window=100000"))
        .json();
    assert_eq!(windowed["contextWindow"], 100_000.0);
    assert_eq!(
        s.get(&format!("/api/sessions/{SESSION}/analysis?window=x"))
            .status,
        400
    );
    assert_eq!(
        s.get(&format!("/api/sessions/{SESSION}?redact=maybe"))
            .status,
        400
    );
}

#[test]
fn missing_session_is_a_typed_404() {
    let s = start(&Options {
        name: "missing",
        ..Options::default()
    });
    let a = s.get("/api/sessions/nope/analysis");
    assert_eq!(a.status, 404);
    assert_eq!(
        a.body,
        r#"{"error":{"_tag":"SessionNotFoundError","id":"nope","searchedRoot":"all agents"}}"#
    );
    let img = s.get(&format!("/api/sessions/{SESSION}/images/0/0"));
    assert_eq!(img.status, 404);
    assert_eq!(tag(&img), "ImageNotFoundError");
    assert_eq!(
        s.get(&format!("/api/sessions/{SESSION}/images/x/0")).status,
        400
    );
}

#[test]
fn settings_round_trip_and_stale_cas() {
    let s = start(&Options {
        name: "settings",
        ..Options::default()
    });
    let first = s.get("/api/settings").json();
    assert_eq!(first["settings"], serde_json::json!({}));
    assert_eq!(first["mtimeMs"], 0);
    assert_eq!(first["path"], s.settings_path.as_str());
    let body =
        r#"{"settings":{"roots":{"claude":[{"path":"~/w","label":"w"}]}},"expectedMtime":0}"#;
    let put = s.call("PUT", "/api/settings", &[], Some(body));
    assert_eq!(put.status, 200);
    let mtime = put.json()["mtimeMs"].as_i64().expect("mtime");
    assert!(mtime > 0);
    let again = s.get("/api/settings").json();
    assert_eq!(again["settings"]["roots"]["claude"][0]["label"], "w");
    let stale = r#"{"settings":{},"expectedMtime":1}"#;
    let refused = s.call("PUT", "/api/settings", &[], Some(stale));
    assert_eq!(refused.status, 409);
    assert_eq!(refused.json()["error"]["reason"], "mtime");
    assert_eq!(s.call("PUT", "/api/settings", &[], Some("{}")).status, 400);
}

#[test]
fn read_only_refuses_writes() {
    let s = start(&Options {
        name: "readonly",
        read_only: true,
        ..Options::default()
    });
    let a = s.call("PUT", "/api/settings", &[], Some(r#"{"settings":{}}"#));
    assert_eq!(a.status, 403);
    assert_eq!(
        a.body,
        r#"{"error":{"_tag":"SettingsWriteError","path":"settings.json","reason":"read-only mode is enabled; refusing to write"}}"#
    );
    assert_eq!(s.get("/api/server").json()["readOnly"], true);
}

#[test]
fn unported_unknown_and_wrong_method() {
    let s = start(&Options {
        name: "routes",
        ..Options::default()
    });
    for (path, feature) in [
        ("/api/stats/report", "stats"),
        ("/api/sessions/x/markers", "stats"),
        ("/api/memory", "memory"),
        ("/api/memory/projects/p/entries", "memory"),
        ("/api/sessions/search?q=x", "search"),
    ] {
        let a = s.get(path);
        assert_eq!(a.status, 501, "{path}");
        assert_eq!(a.json()["error"]["feature"], feature, "{path}");
    }
    let unknown = s.get("/api/nope");
    assert_eq!(unknown.status, 404);
    assert_eq!(tag(&unknown), "NotFoundError");
    let wrong = s.call("POST", "/api/sessions", &[], None);
    assert_eq!(wrong.status, 405);
    assert_eq!(tag(&wrong), "MethodNotAllowedError");
    assert_eq!(s.get("/api/watch").body, r#"{"memory":0,"sessions":0}"#);
    assert_eq!(s.get("/health").body, "ok");
    let spa = s.get("/sessions/abc");
    assert_eq!(spa.status, 200);
    assert_eq!(spa.header("cache-control"), Some("no-store"));
}
