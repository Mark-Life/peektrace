//! A list row shows the same cost as the session detail, for every agent:
//! the agent's record when it covers the session, else the estimate with
//! subagents and workflow agents. Synthetic transcripts only.

mod common;

use std::fs;
use std::path::Path;

use common::{create_db, fixture, path_str, session_env, temp_dir};
use peektrace_core::header_cache::HeaderCache;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_core::sessions::service::{Target, analyze_session};
use peektrace_core::{
    Agent, AgentRoots, ListFilter, SessionHeader, list_sessions, list_sessions_cached,
};
use rusqlite::params;
use serde_json::{Value, json};

/// 2026-01-01T00:00:00Z in epoch ms.
const T0_MS: i64 = 1_767_225_600_000;

fn close(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-9 * a.abs().max(1.0),
        (a, b) => a == b,
    }
}

/// Every listed row against its detail; returns the rows per agent.
fn assert_list_matches_detail(roots: &AgentRoots, headers: &[SessionHeader]) -> Vec<Agent> {
    for h in headers {
        let a = analyze_session(session_env(roots), Target::Listed(h), None, false)
            .unwrap_or_else(|e| panic!("{}: {e}", h.id));
        let detail = a.cost.as_ref();
        let id = h.id.as_str();
        assert!(
            close(h.cost.usd, detail.and_then(|c| c.total_usd)),
            "{id}: list {:?}, detail {:?}",
            h.cost.usd,
            detail.and_then(|c| c.total_usd)
        );
        assert_eq!(
            h.cost.from_agent,
            detail.is_some_and(|c| c.from_agent),
            "{id}"
        );
        if !h.cost.from_agent {
            assert!(
                close(h.cost.subagents_usd, detail.and_then(|c| c.subagents_usd)),
                "{id}"
            );
            assert_eq!(
                h.cost.unpriced_models,
                detail.and_then(|c| c.unpriced_models.clone()),
                "{id}"
            );
        }
    }
    headers.iter().map(|h| h.agent).collect()
}

fn write_jsonl(path: &Path, rows: &[Value]) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let text: Vec<String> = rows.iter().map(Value::to_string).collect();
    fs::write(path, text.join("\n") + "\n").expect("write");
}

/// One call of `input` tokens; `sidechain` marks an inline subagent line.
fn call(request: &str, model: &str, input: u64, sidechain: bool) -> Value {
    json!({"type":"assistant","timestamp":"2026-01-01T00:00:01Z","requestId":request,
        "isSidechain":sidechain,"message":{"model":model,"usage":{"input_tokens":input,
        "output_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},
        "content":[{"type":"text","text":"ok"}]}})
}

fn cost_state(session: &str, start_ms: i64, total: f64) -> Value {
    json!({"type":"cost-state","sessionId":session,"totalCostUSD":total,"startTime":start_ms,
        "modelUsage":{},"hasUnknownModelCost":false})
}

/// A Claude session `id` in `slug_dir`, with a subagent and a workflow agent.
fn claude_session(slug_dir: &Path, id: &str, extra: &[Value]) {
    let mut rows = vec![
        json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","sessionId":id,
            "message":{"content":"hi"}}),
        call("r1", "claude-opus-5-5", 100_000, false),
        call("r2", "claude-haiku-4-5", 50_000, true),
    ];
    rows.extend_from_slice(extra);
    write_jsonl(&slug_dir.join(format!("{id}.jsonl")), &rows);
    let subagents = slug_dir.join(id).join("subagents");
    write_jsonl(
        &subagents.join("agent-a1.jsonl"),
        &[
            call("s1", "claude-sonnet-4-5", 200_000, true),
            call("s2", "mystery-model", 10, true),
        ],
    );
    write_jsonl(
        &subagents.join("workflows/wf_run1/agent-w1.jsonl"),
        &[call("w1", "claude-opus-5-5", 300_000, true)],
    );
}

fn claude_roots(projects: &Path) -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(path_str(projects)),
        ..RootsEnv::default()
    };
    AgentRoots::resolve(&env, &Settings::default())
}

fn claude_only() -> ListFilter {
    ListFilter {
        agent: Some(Agent::Claude),
        project: None,
        with_subagent_costs: true,
    }
}

/// Every agent, costed as `sessions ls` does.
fn every_agent() -> ListFilter {
    ListFilter {
        with_subagent_costs: true,
        ..ListFilter::default()
    }
}

fn by_id<'a>(headers: &'a [SessionHeader], id: &str) -> &'a SessionHeader {
    headers
        .iter()
        .find(|h| h.id.as_str() == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

#[test]
fn claude_rows_match_the_detail_with_and_without_a_record() {
    let projects = temp_dir("list-cost-claude");
    let slug = projects.join("-p");
    // No record: main + inline sidechain + subagent + workflow agent.
    claude_session(&slug, "none", &[]);
    // A full record: it is the total, subagents already in it.
    claude_session(&slug, "full", &[cost_state("full", T0_MS - 1000, 9.5)]);
    // A record from a resumed process only: the estimate is the total.
    claude_session(
        &slug,
        "late",
        &[cost_state("late", T0_MS + 3_600_000, 0.25)],
    );
    // A forked copy of another session's record does not count.
    claude_session(&slug, "fork", &[cost_state("other", T0_MS - 1000, 7.0)]);
    let roots = claude_roots(&projects);
    let listing = list_sessions(&roots, &claude_only());
    assert_eq!(
        assert_list_matches_detail(&roots, &listing.headers).len(),
        4
    );
    let h = &listing.headers;
    assert_eq!(by_id(h, "full").cost.usd, Some(9.5));
    assert_eq!(by_id(h, "full").cost.subagents_usd, None, "not read");
    assert_eq!(by_id(h, "full").cost.unpriced_models, None);
    for id in ["none", "late", "fork"] {
        let c = &by_id(h, id).cost;
        assert!(!c.from_agent, "{id}");
        assert!(c.subagents_usd.is_some_and(|s| s > 1.0), "{id}");
        assert_eq!(
            c.unpriced_models,
            Some(vec!["mystery-model".to_owned()]),
            "{id}"
        );
    }
}

#[test]
fn cached_subagent_costs_follow_file_changes() {
    let projects = temp_dir("list-cost-cache");
    let slug = projects.join("-p");
    claude_session(&slug, "s", &[]);
    let roots = claude_roots(&projects);
    let cache = HeaderCache::default();
    let first = list_sessions_cached(&roots, &claude_only(), &cache).headers;
    let again = list_sessions_cached(&roots, &claude_only(), &cache).headers;
    assert_eq!(first, again);
    write_jsonl(
        &slug.join("s/subagents/agent-a2.jsonl"),
        &[call("n1", "claude-opus-5-5", 1_000_000, true)],
    );
    let grown = list_sessions_cached(&roots, &claude_only(), &cache).headers;
    let added = grown[0].cost.usd.unwrap_or(0.0) - first[0].cost.usd.unwrap_or(0.0);
    assert!(
        (added - 4.0).abs() < 0.01,
        "a new subagent adds its cost: {added}"
    );
    assert_list_matches_detail(&roots, &grown);
}

#[test]
fn fixture_rows_match_the_detail() {
    let pi_root = temp_dir("list-cost-pi");
    let pi_slug = pi_root.join("--tmp-pi--");
    fs::create_dir_all(&pi_slug).expect("mkdir");
    for entry in fs::read_dir(fixture("sessions-pi")).expect("dir").flatten() {
        fs::copy(entry.path(), pi_slug.join(entry.file_name())).expect("copy");
    }
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(fixture("sessions/projects")),
        codex_sessions: Some(fixture("sessions-codex")),
        pi_sessions: Some(path_str(&pi_root)),
        ..RootsEnv::default()
    };
    let roots = AgentRoots::resolve(&env, &Settings::default());
    let listing = list_sessions(&roots, &every_agent());
    let agents = assert_list_matches_detail(&roots, &listing.headers);
    for agent in [Agent::Claude, Agent::Codex, Agent::Pi] {
        assert!(agents.contains(&agent), "{agent:?}");
    }
    let codex: Vec<_> = listing
        .headers
        .iter()
        .filter(|h| h.agent == Agent::Codex)
        .collect();
    assert!(
        codex.iter().all(|h| h.cost.usd.is_some()),
        "codex is priced"
    );
}

/// One Pi call with Pi's own `usage.cost.total`.
fn pi_call(id: &str, recorded: f64) -> Value {
    json!({"type":"message","id":id,"timestamp":"2026-01-01T00:00:01Z","message":{
        "role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"ok"}],
        "usage":{"input":1000,"output":10,"cacheRead":0,"cacheWrite":0,
        "cost":{"total":recorded}}}})
}

#[test]
fn pi_rows_match_the_detail_with_and_without_a_record() {
    let root = temp_dir("list-cost-pi-record");
    let slug = root.join("--tmp-x--");
    let session = |id: &str, calls: &[Value]| {
        let mut rows = vec![json!({"type":"session","id":id,
            "timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp/x"})];
        rows.extend_from_slice(calls);
        write_jsonl(&slug.join(format!("2026-01-01_{id}.jsonl")), &rows);
    };
    session("full", &[pi_call("a", 0.5), pi_call("b", 0.25)]);
    session("part", &[pi_call("a", 0.5), pi_call("b", 0.0)]);
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        pi_sessions: Some(path_str(&root)),
        ..RootsEnv::default()
    };
    let roots = AgentRoots::resolve(&env, &Settings::default());
    let listing = list_sessions(&roots, &every_agent());
    assert_eq!(
        assert_list_matches_detail(&roots, &listing.headers).len(),
        2
    );
    let full = listing.headers.iter().find(|h| h.cost.from_agent);
    assert_eq!(full.and_then(|h| h.cost.usd), Some(0.75));
}

#[test]
fn opencode_rows_match_the_detail_with_and_without_a_record() {
    let dir = temp_dir("list-cost-oc");
    let db = create_db(&dir.join("opencode.db"));
    for (id, cost) in [("ses_rec", Some(0.42)), ("ses_est", None)] {
        db.execute(
            "INSERT INTO session (id, directory, title, version, time_created, time_updated, cost) VALUES (?, '/tmp/x', 't', '1.18.2', 1784148745485, 1784148748261, ?)",
            params![id, cost],
        )
        .expect("session");
        db.execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, 1784148745500, ?)",
            params![
                format!("msg_{id}"),
                id,
                r#"{"role":"assistant","modelID":"claude-opus-5-5","providerID":"anthropic","tokens":{"input":1000,"output":10,"reasoning":0,"cache":{"write":0,"read":0}},"time":{"created":1784148745500}}"#
            ],
        )
        .expect("message");
    }
    drop(db);
    let roots = common::opencode_roots(&path_str(&dir));
    let listing = list_sessions(&roots, &every_agent());
    assert_eq!(
        assert_list_matches_detail(&roots, &listing.headers).len(),
        2
    );
    assert!(listing.headers.iter().any(|h| h.cost.from_agent));
    assert!(listing.headers.iter().any(|h| !h.cost.from_agent));
}

#[test]
fn the_same_id_in_two_projects_keeps_its_own_subagents() {
    let projects = temp_dir("list-cost-dup");
    claude_session(&projects.join("-a"), "dup", &[]);
    fs::remove_dir_all(projects.join("-a/dup")).expect("rm");
    claude_session(&projects.join("-b"), "dup", &[]);
    let roots = claude_roots(&projects);
    let listing = list_sessions(&roots, &claude_only());
    assert_eq!(
        assert_list_matches_detail(&roots, &listing.headers).len(),
        2
    );
    let [a, b] = listing.headers.as_slice() else {
        panic!("two rows")
    };
    assert_ne!(a.cost.usd, b.cost.usd, "only one has subagent files");
}

#[cfg(unix)]
#[test]
fn a_cached_subagent_that_stops_opening_counts_as_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let projects = temp_dir("list-cost-chmod");
    claude_session(&projects.join("-p"), "s", &[]);
    let file = projects.join("-p/s/subagents/agent-a0.jsonl");
    fs::rename(projects.join("-p/s/subagents/agent-a1.jsonl"), &file).expect("mv");
    let roots = claude_roots(&projects);
    let cache = HeaderCache::default();
    let before = list_sessions_cached(&roots, &claude_only(), &cache).headers;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("chmod");
    if fs::File::open(&file).is_ok() {
        return; // running as root: the file still opens
    }
    let after = list_sessions_cached(&roots, &claude_only(), &cache).headers;
    let uncached = list_sessions(&roots, &claude_only()).headers;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod back");
    assert_eq!(after[0].cost, uncached[0].cost);
    assert_ne!(after[0].cost, before[0].cost);
}

#[test]
fn a_list_that_skips_subagents_leaves_their_cost_unknown() {
    let projects = temp_dir("list-cost-skip");
    let slug = projects.join("-p");
    claude_session(&slug, "none", &[]);
    claude_session(&slug, "full", &[cost_state("full", T0_MS - 1000, 9.5)]);
    let roots = claude_roots(&projects);
    let filter = ListFilter {
        with_subagent_costs: false,
        ..claude_only()
    };
    let h = list_sessions(&roots, &filter).headers;
    assert_eq!(by_id(&h, "none").cost.usd, None);
    assert_eq!(by_id(&h, "full").cost.usd, Some(9.5));
}
