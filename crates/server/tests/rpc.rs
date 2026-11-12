//! `packages/rpc/test/rpc.test.ts` over `/api`: a Claude root holding a copy
//! of the session fixture and a memory vault, nothing else.

mod common;

use std::path::{Path, PathBuf};

use common::{Options, SESSION, start};
use peektrace_core::AgentRoots;
use peektrace_core::roots::Source;
use serde_json::{Value, json};

const SESSION_SLUG: &str = "-Users-demo-proj";
const MEM_SLUG: &str = "-tmp-mem";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("ls") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}

/// Seed a temp root: the session fixture plus a memory vault.
fn seed(name: &str) -> PathBuf {
    let base = common::temp_dir(&format!("rpc-root-{name}"));
    let fixture = PathBuf::from(common::fixtures("sessions/projects")).join(SESSION_SLUG);
    copy_dir(&fixture, &base.join(SESSION_SLUG));
    let mem = base.join(MEM_SLUG).join("memory");
    std::fs::create_dir_all(&mem).expect("mkdir");
    std::fs::write(
        mem.join("alpha.md"),
        "---\nname: alpha\ndescription: First\ntype: user\n---\nLinks [[beta]].\n",
    )
    .expect("alpha");
    std::fs::write(mem.join("MEMORY.md"), "- [Alpha](alpha.md) — first\n").expect("index");
    base
}

fn claude_only(home: &Path, projects_root: &Path) -> AgentRoots {
    AgentRoots {
        claude: vec![Source {
            id: "claude".to_owned(),
            label: "default".to_owned(),
            home: home.to_string_lossy().into_owned(),
            projects_root: projects_root.to_string_lossy().into_owned(),
        }],
        ..AgentRoots::default()
    }
}

fn server(name: &str, read_only: bool) -> (common::TestServer, PathBuf) {
    let root = seed(name);
    let s = start(&Options {
        name,
        read_only,
        roots: Some(claude_only(&root.join("home"), &root)),
        ..Options::default()
    });
    (s, root)
}

fn tag(v: &Value) -> &str {
    v["error"]["_tag"].as_str().unwrap_or_default()
}

fn ids(v: &Value) -> Vec<String> {
    v["headers"]
        .as_array()
        .expect("headers")
        .iter()
        .map(|h| h["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

// describe("capabilities")

#[test]
fn list_returns_the_matrix_with_memory_crud_supported_for_claude() {
    let (s, _) = server("caps", false);
    let caps = s.get("/api/capabilities").json();
    let crud = caps
        .as_array()
        .expect("caps")
        .iter()
        .find(|c| c["id"] == "memory.crud")
        .expect("memory.crud");
    assert_eq!(crud["perAgent"]["claude"]["level"], "supported");
    assert_eq!(crud["perAgent"]["codex"]["level"], "planned");
}

// describe("server.info")

#[test]
fn reports_write_mode_and_the_scanned_claude_root() {
    let (s, root) = server("info", false);
    let info = s.get("/api/server").json();
    assert_eq!(info["readOnly"], false);
    assert_ne!(info["userHome"].as_str().unwrap_or_default(), "");
    let claude = info["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|a| a["agent"] == "claude")
        .expect("claude");
    assert_eq!(
        claude["roots"],
        json!([{
            "isDefault": true,
            "home": root.join("home").to_string_lossy(),
            "path": root.to_string_lossy(),
            "exists": true,
        }])
    );
    // Rust always lists all four agents; the others have no roots here.
    for a in &info["agents"].as_array().expect("agents")[1..] {
        assert_eq!(a["roots"], json!([]), "{}", a["agent"]);
    }
}

#[test]
fn reports_read_only_mode() {
    let (s, _) = server("info-ro", true);
    assert_eq!(s.get("/api/server").json()["readOnly"], true);
}

#[test]
fn flags_a_root_missing_on_disk() {
    let root = seed("info-missing");
    let missing = root.join("no-such-dir");
    let s = start(&Options {
        name: "info-missing",
        roots: Some(claude_only(&root.join("home"), &missing)),
        ..Options::default()
    });
    let info = s.get("/api/server").json();
    let first = &info["agents"][0]["roots"][0];
    assert_eq!(first["path"], missing.to_string_lossy().as_ref());
    assert_eq!(first["exists"], false);
}

// describe("sessions")

#[test]
fn list_returns_the_fixture_header() {
    let (s, _) = server("list", false);
    let listing = s.get("/api/sessions").json();
    let headers = listing["headers"].as_array().expect("headers");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0]["id"], SESSION);
    assert_eq!(headers[0]["project"], SESSION_SLUG);
    assert_eq!(headers[0]["model"], "claude-opus-4");
}

#[test]
fn listing_matches_list_and_reports_no_unreadable_sessions() {
    let (s, _) = server("listing", false);
    let listing = s.get("/api/sessions").json();
    assert_eq!(ids(&listing), [SESSION]);
    assert_eq!(listing["unreadable"], json!([]));
}

#[test]
fn list_filters_by_project() {
    let (s, _) = server("project", false);
    let none = s.get(&format!("/api/sessions?project={MEM_SLUG}")).json();
    assert_eq!(ids(&none), Vec::<String>::new());
}

#[test]
fn list_filters_by_agent() {
    let (s, _) = server("agent", false);
    assert_eq!(ids(&s.get("/api/sessions?agent=claude").json()), [SESSION]);
    assert_eq!(
        ids(&s.get("/api/sessions?agent=codex").json()),
        Vec::<String>::new()
    );
}

#[test]
fn get_folds_the_subagent_transcript() {
    let (s, _) = server("get", false);
    let parsed = s.get(&format!("/api/sessions/{SESSION}")).json();
    let subagents = parsed["subagents"].as_array().expect("subagents");
    assert_eq!(subagents.len(), 1);
    assert_eq!(subagents[0]["id"], "agent-1");
}

#[test]
fn analyze_reproduces_the_golden_peak_and_budget() {
    let (s, _) = server("golden", false);
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(common::fixtures("sessions/golden.json")).expect("golden"),
    )
    .expect("golden json");
    let a = s.get(&format!("/api/sessions/{SESSION}/analysis")).json();
    assert_eq!(
        a["peakContextTokens"].as_f64(),
        golden["peakContextTokens"].as_f64()
    );
    let budget = |v: &Value| -> Vec<(String, f64)> {
        v["budget"]
            .as_array()
            .expect("budget")
            .iter()
            .map(|b| {
                let key = b["key"]
                    .as_str()
                    .map_or_else(|| b["key"].to_string(), str::to_owned);
                (key, b["tokens"].as_f64().unwrap_or(f64::NAN))
            })
            .collect()
    };
    assert_eq!(budget(&a), budget(&golden));
}

#[test]
fn search_finds_a_known_phrase_with_its_line_and_a_snippet() {
    let (s, _) = server("search-hit", false);
    let a = s.get("/api/sessions/search?q=refactor%20the%20AUTH%20module");
    assert_eq!(a.status, 200);
    let r = a.json();
    let hit = r["sessions"]
        .as_array()
        .and_then(|v| v.iter().find(|x| x["id"] == SESSION))
        .cloned()
        .unwrap_or_default();
    assert_eq!(hit["hits"][0]["line"], 1);
    let snippet = hit["hits"][0]["snippet"].as_str().unwrap_or_default();
    assert!(snippet.contains("refactor the auth module"), "{snippet}");
}

#[test]
fn search_with_no_match_returns_an_empty_result() {
    let (s, _) = server("search-miss", false);
    let r = s.get("/api/sessions/search?q=no-such-phrase-xyz").json();
    assert_eq!(r["sessions"], json!([]));
    assert_eq!(r["truncated"], false);
}

#[test]
fn search_rejects_a_blank_query_a_negative_limit_and_a_bad_since() {
    let (s, _) = server("search-bad", false);
    for q in [
        "q=%20%20",
        "q=x&limit=-1",
        "q=x&limit=1.5",
        "q=x&since=7d",
        "q=x&since=yesterday",
        "q=x&agent=all",
        "since=2020-01-01",
    ] {
        let a = s.get(&format!("/api/sessions/search?{q}"));
        assert_eq!(a.status, 400, "{q}");
        assert_eq!(tag(&a.json()), "BadRequestError", "{q}");
    }
}

#[test]
fn analyze_of_a_missing_session_fails_with_the_typed_wire_error() {
    let (s, _) = server("missing", false);
    let a = s.get("/api/sessions/does-not-exist/analysis");
    assert_eq!(a.status, 404);
    assert_eq!(tag(&a.json()), "SessionNotFoundError");
}

// describe("memory")

const ENTRIES: &str = "/api/memory/projects/-tmp-mem/entries";

#[test]
fn projects_all_vaults_and_vault_return_the_seeded_vault() {
    let (s, _) = server("mem-read", false);
    let projects = s.get("/api/memory/projects").json();
    assert!(
        projects
            .as_array()
            .is_some_and(|p| p.iter().any(|x| x["slug"] == MEM_SLUG))
    );
    let all = s.get("/api/memory").json();
    assert!(
        all["vaults"]
            .as_array()
            .is_some_and(|v| v.iter().any(|x| x["slug"] == MEM_SLUG))
    );
    let vault = s.get(&format!("/api/memory/projects/{MEM_SLUG}")).json();
    let slugs: Vec<_> = vault["entries"]
        .as_array()
        .map(|e| e.iter().map(|x| x["slug"].clone()).collect())
        .unwrap_or_default();
    assert!(slugs.contains(&json!("alpha")));
}

#[test]
fn create_writes_an_entry_and_indexes_it() {
    let (s, root) = server("mem-create", false);
    let body =
        r#"{"name":"gamma","description":"Third","type":"reference","body":"Gamma body.\n"}"#;
    let a = s.call("POST", ENTRIES, &[], Some(body));
    assert_eq!(a.status, 200, "{}", a.body);
    assert_eq!(a.json()["slug"], "gamma");
    assert_eq!(a.json()["inIndex"], true);
    assert!(root.join(MEM_SLUG).join("memory/gamma.md").exists());
}

#[test]
fn a_stale_cas_update_surfaces_file_changed_error() {
    let (s, _) = server("mem-cas", false);
    let body = r#"{"name":"cas","description":"CAS target","type":"user","body":"v1\n"}"#;
    assert_eq!(s.call("POST", ENTRIES, &[], Some(body)).status, 200);
    let stale = r#"{"body":"v2\n","expectedMtime":1}"#;
    let a = s.call("PATCH", &format!("{ENTRIES}/cas"), &[], Some(stale));
    assert_eq!(a.status, 409);
    assert_eq!(tag(&a.json()), "FileChangedError");
    assert_eq!(a.json()["error"]["reason"], "mtime");
}

#[test]
fn a_bad_name_is_a_memory_validation_error() {
    let (s, _) = server("mem-bad", false);
    let body = r#"{"name":"Not Kebab","description":"bad","type":"user","body":"x"}"#;
    let a = s.call("POST", ENTRIES, &[], Some(body));
    assert_eq!(a.status, 422);
    assert_eq!(tag(&a.json()), "MemoryValidationError");
    assert_eq!(a.json()["error"]["name"], "Not Kebab");
}

#[test]
fn delete_removes_the_file() {
    let (s, root) = server("mem-delete", false);
    let a = s.call("DELETE", &format!("{ENTRIES}/alpha"), &[], None);
    assert_eq!(a.status, 200, "{}", a.body);
    assert_eq!(a.json()["slug"], "alpha");
    assert!(!root.join(MEM_SLUG).join("memory/alpha.md").exists());
}

#[test]
fn read_only_refuses_every_memory_write_and_leaves_the_disk_alone() {
    let (s, root) = server("mem-ro", true);
    let dir = root.join(MEM_SLUG).join("memory");
    let before = std::fs::read_to_string(dir.join("MEMORY.md")).unwrap_or_default();
    let create = r#"{"name":"gamma","description":"Third","type":"reference","body":"x"}"#;
    for (method, path, body) in [
        ("POST", ENTRIES.to_owned(), Some(create)),
        ("PATCH", format!("{ENTRIES}/alpha"), Some(r#"{"body":"y"}"#)),
        ("DELETE", format!("{ENTRIES}/alpha"), None),
    ] {
        let a = s.call(method, &path, &[], body);
        assert_eq!(a.status, 403, "{method}");
        assert_eq!(
            a.json(),
            json!({"error": {"_tag": "MemoryValidationError", "reason": "read-only mode is enabled; refusing to write"}})
        );
    }
    assert!(!dir.join("gamma.md").exists());
    assert!(dir.join("alpha.md").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("MEMORY.md")).unwrap_or_default(),
        before
    );
    assert_eq!(s.call("POST", "/api/stats/refresh", &[], None).status, 200);
}

#[test]
fn bad_memory_bodies_are_bad_requests() {
    let (s, _) = server("mem-body", false);
    for body in [
        "",
        "[]",
        r#"{"name":"x","description":"d","type":"user"}"#,
        r#"{"name":1,"description":"d","type":"user","body":"b"}"#,
    ] {
        let a = s.call("POST", ENTRIES, &[], Some(body));
        assert_eq!(a.status, 400, "{body}");
        assert_eq!(tag(&a.json()), "BadRequestError");
    }
    let a = s.call(
        "PATCH",
        &format!("{ENTRIES}/alpha"),
        &[],
        Some(r#"{"frontmatter":"x"}"#),
    );
    assert_eq!(a.status, 400);
}

#[test]
fn memory_routes_answer_405_for_other_methods() {
    let (s, _) = server("mem-405", false);
    for (method, path) in [
        ("POST", "/api/memory".to_owned()),
        ("GET", ENTRIES.to_owned()),
        ("GET", format!("{ENTRIES}/alpha")),
        ("GET", "/api/stats/refresh".to_owned()),
        ("POST", "/api/stats/report".to_owned()),
    ] {
        let a = s.call(method, &path, &[], None);
        assert_eq!(a.status, 405, "{method} {path}");
        assert_eq!(tag(&a.json()), "MethodNotAllowedError");
    }
}

// describe("settings")

#[test]
fn get_update_get_roundtrips_the_settings_file() {
    let (s, _) = server("settings-rt", false);
    let empty = s.get("/api/settings").json();
    assert_eq!(empty["settings"], json!({}));
    assert_eq!(empty["mtimeMs"], 0);
    let body = json!({
        "settings": {"roots": {"claude": [{"path": "/work/.claude", "label": "work"}]}},
        "expectedMtime": empty["mtimeMs"],
    })
    .to_string();
    let written = s.call("PUT", "/api/settings", &[], Some(&body));
    assert_eq!(written.status, 200);
    assert!(written.json()["mtimeMs"].as_i64().unwrap_or(0) > 0);
    let reread = s.get("/api/settings").json();
    assert_eq!(
        reread["settings"]["roots"]["claude"],
        json!([{"path": "/work/.claude", "label": "work"}])
    );
}

#[test]
fn a_stale_settings_cas_update_surfaces_file_changed_error() {
    let (s, _) = server("settings-cas", false);
    let first = r#"{"settings":{"roots":{"codex":[{"path":"/a"}]}}}"#;
    assert_eq!(s.call("PUT", "/api/settings", &[], Some(first)).status, 200);
    let stale = r#"{"settings":{"roots":{"codex":[{"path":"/b"}]}},"expectedMtime":1}"#;
    let refused = s.call("PUT", "/api/settings", &[], Some(stale));
    assert_eq!(refused.status, 409);
    assert_eq!(tag(&refused.json()), "FileChangedError");
}

// describe("filterListing"): an unreadable Codex row has project "" in Rust,
// so the "demo"-project row of the TS fixture cannot occur.

#[cfg(unix)]
#[test]
fn an_unreadable_row_with_no_known_project_passes_a_project_filter_but_not_the_agent_filter() {
    use std::os::unix::fs::PermissionsExt as _;
    let base = common::temp_dir("rpc-unreadable");
    let file = base.join("codex/2026/01/02/rollout-locked.jsonl");
    std::fs::create_dir_all(file.parent().expect("dir")).expect("mkdir");
    std::fs::write(&file, "{}\n").expect("write");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&file).is_ok() {
        // Running as root: every file is readable.
        return;
    }
    let roots = AgentRoots {
        codex: vec![Source {
            id: "codex".to_owned(),
            label: "default".to_owned(),
            home: base.to_string_lossy().into_owned(),
            projects_root: base.join("codex").to_string_lossy().into_owned(),
        }],
        ..AgentRoots::default()
    };
    let s = start(&Options {
        name: "rpc-unreadable-server",
        roots: Some(roots),
        ..Options::default()
    });
    let kept = s.get("/api/sessions?project=demo").json();
    let rows: Vec<_> = kept["unreadable"]
        .as_array()
        .expect("unreadable")
        .iter()
        .map(|u| (u["agent"].clone(), u["project"].clone()))
        .collect();
    assert_eq!(rows, [(json!("codex"), json!(""))]);
    let claude = s.get("/api/sessions?project=demo&agent=claude").json();
    assert_eq!(claude["unreadable"], json!([]));
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
}
