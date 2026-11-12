//! The static support matrix (`capabilities.test.ts`).

use peektrace_core::capabilities::{SupportLevel, list_capabilities};

#[test]
fn rows_in_matrix_order() {
    let ids: Vec<_> = list_capabilities().iter().map(|c| c.id).collect();
    assert_eq!(
        ids,
        [
            "session.view",
            "session.debug-context",
            "memory.view",
            "memory.crud",
            "mcp.dashboard",
            "skills.browser",
            "file-history.diff"
        ]
    );
}

#[test]
fn memory_is_claude_only() {
    let crud = list_capabilities()
        .iter()
        .find(|c| c.id == "memory.crud")
        .expect("memory.crud");
    assert_eq!(crud.per_agent[0].level, SupportLevel::Supported);
    assert_eq!(
        crud.per_agent[0].note,
        Some("Claude markdown memories only")
    );
    assert!(
        crud.per_agent[1..]
            .iter()
            .all(|s| s.level == SupportLevel::Planned && s.note.is_none())
    );
}

#[test]
fn session_view_supported_everywhere() {
    let view = &list_capabilities()[0];
    assert!(
        view.per_agent
            .iter()
            .all(|s| s.level == SupportLevel::Supported)
    );
    let debug = &list_capabilities()[1];
    assert_eq!(debug.per_agent[3].level, SupportLevel::Partial);
    assert!(
        debug.per_agent[3]
            .note
            .is_some_and(|n| n.contains("inferred"))
    );
}

// capabilities.test.ts, same assertions.

use peektrace_core::Agent;

fn find(id: &str) -> &'static peektrace_core::capabilities::Capability {
    list_capabilities()
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("{id}"))
}

fn level_of(id: &str, agent: Agent) -> SupportLevel {
    let at = Agent::ALL.iter().position(|a| *a == agent).expect("agent");
    find(id).per_agent[at].level
}

#[test]
fn every_capability_is_exhaustive_over_agents() {
    for cap in list_capabilities() {
        assert_eq!(cap.per_agent.len(), Agent::ALL.len(), "{}", cap.id);
    }
}

#[test]
fn seeds_the_committed_claude_surfaces_as_supported() {
    for id in ["memory.view", "memory.crud"] {
        assert_eq!(level_of(id, Agent::Claude), SupportLevel::Supported);
        for agent in [Agent::Codex, Agent::Pi, Agent::Opencode] {
            assert_eq!(level_of(id, agent), SupportLevel::Planned, "{id}");
        }
    }
    for agent in Agent::ALL {
        assert_eq!(level_of("session.view", agent), SupportLevel::Supported);
    }
    assert_eq!(
        level_of("session.debug-context", Agent::Claude),
        SupportLevel::Supported
    );
    for agent in [Agent::Codex, Agent::Pi, Agent::Opencode] {
        assert_eq!(
            level_of("session.debug-context", agent),
            SupportLevel::Partial
        );
    }
}

/// TS `CapabilityRegistry.supports`: the capability exists and is fully
/// supported for the agent.
fn supports(id: &str, agent: Agent) -> bool {
    let at = Agent::ALL.iter().position(|a| *a == agent).expect("agent");
    list_capabilities()
        .iter()
        .find(|c| c.id == id)
        .is_some_and(|c| c.per_agent[at].level == SupportLevel::Supported)
}

#[test]
fn supports_gates_writes_to_claude_only() {
    assert!(supports("memory.crud", Agent::Claude));
    assert!(!supports("memory.crud", Agent::Codex));
    assert!(!supports("unknown.cap", Agent::Claude));
}
