//! Listing against the TS test fixtures in `packages/core/test/fixtures`.
//! Expected values come from the TS CLI (`sessions ls --json`).

use std::path::PathBuf;

use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, claude_sources};
use peektrace_core::select::{SelectOptions, select_sessions};
use peektrace_core::{Agent, AgentRoots, ListFilter, SessionHeader, list_sessions};

fn fixtures(sub: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/core/test/fixtures");
    root.join(sub).to_string_lossy().into_owned()
}

fn roots_at(projects: &str) -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(projects.to_owned()),
        ..RootsEnv::default()
    };
    AgentRoots {
        claude: claude_sources(&env, &[]),
    }
}

fn claude_only(project: Option<&str>) -> ListFilter {
    ListFilter {
        agent: Some(Agent::Claude),
        project: project.map(str::to_owned),
    }
}

fn find<'a>(headers: &'a [SessionHeader], id: &str) -> &'a SessionHeader {
    headers
        .iter()
        .find(|h| h.id.as_str() == id)
        .unwrap_or_else(|| panic!("no header {id}"))
}

#[test]
fn golden_session_header() {
    let listing = list_sessions(
        &roots_at(&fixtures("sessions/projects")),
        &claude_only(None),
    );
    assert_eq!(listing.unreadable, Vec::new());
    let [h] = listing.headers.as_slice() else {
        panic!("expected one header, got {}", listing.headers.len())
    };
    assert_eq!(h.id.as_str(), "11111111-1111-4111-8111-111111111111");
    assert_eq!(h.project.as_str(), "-Users-demo-proj");
    assert_eq!(h.message_count, 6);
    assert_eq!(h.size_bytes, 2552);
    assert_eq!(h.updated_at, "2026-06-01T10:00:30.000Z");
    assert_eq!(h.started_at.as_deref(), Some("2026-06-01T10:00:00.000Z"));
    assert_eq!(h.cwd.as_deref(), Some("/Users/demo/proj"));
    assert_eq!(h.git_branch.as_deref(), Some("main"));
    assert_eq!(h.model.as_deref(), Some("claude-opus-4"));
    assert_eq!(h.title.as_deref(), Some("Refactor auth module"));
    assert_eq!(h.cost_usd, Some(0.117_038_5));
    assert_eq!(h.source, None);
}

#[test]
fn claude_fixture_costs_and_counts() {
    let listing = list_sessions(
        &roots_at(&fixtures("")),
        &claude_only(Some("sessions-claude")),
    );
    let h = &listing.headers;
    assert_eq!(h.len(), 7);
    assert!(
        h.iter()
            .all(|row| row.project.as_str() == "sessions-claude")
    );
    let cases = [
        ("thinking-tokens", 4, Some(0.039_082)),
        ("server-tools", 2, Some(0.016_64)),
        ("streamed-output", 3, Some(0.028_782_000_000_000_002)),
        ("measured-image", 11, Some(0.320_623_750_000_000_04)),
        ("prompt-snapshot", 3, Some(0.043_480_000_000_000_005)),
        ("prompt-origin", 6, None),
        ("retries", 22, None),
    ];
    for (id, count, cost) in cases {
        let row = find(h, id);
        assert_eq!(row.message_count, count, "{id}");
        assert_eq!(row.cost_usd, cost, "{id}");
    }
    assert_eq!(
        find(h, "retries").unpriced_models,
        Some(vec!["claude-test".to_owned()])
    );
}

#[test]
fn subagents_are_not_listed() {
    let listing = list_sessions(
        &roots_at(&fixtures("sessions/projects")),
        &claude_only(None),
    );
    assert!(
        listing
            .headers
            .iter()
            .all(|h| !h.path.contains("subagents"))
    );
}

#[test]
fn other_agents_list_nothing() {
    let filter = ListFilter {
        agent: Some(Agent::Codex),
        project: None,
    };
    let listing = list_sessions(&roots_at(&fixtures("")), &filter);
    assert_eq!(listing.headers, Vec::new());
}

#[test]
fn selection_sorts_newest_first_and_limits() {
    let listing = list_sessions(
        &roots_at(&fixtures("")),
        &claude_only(Some("sessions-claude")),
    );
    let picked = select_sessions(
        listing.headers,
        &SelectOptions {
            limit: 3,
            ..SelectOptions::default()
        },
        &Utc,
    );
    let ids: Vec<_> = picked.rows.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, ["prompt-origin", "thinking-tokens", "server-tools"]);
    assert_eq!(picked.hidden, 4);
}

#[test]
fn missing_root_is_empty() {
    let listing = list_sessions(&roots_at("/nonexistent/projects"), &claude_only(None));
    assert_eq!(listing.headers.len() + listing.unreadable.len(), 0);
}
