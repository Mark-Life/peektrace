//! The static feature x agent support matrix. The inspector shows it, and
//! writes check it before they touch disk.

use crate::model::Agent;

/// Capability id that gates memory writes.
pub const MEMORY_CRUD: &str = "memory.crud";

/// How well an agent supports a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportLevel {
    Supported,
    Partial,
    Planned,
    Unsupported,
}

impl SupportLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partial => "partial",
            Self::Planned => "planned",
            Self::Unsupported => "unsupported",
        }
    }
}

/// One agent's support for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Support {
    pub level: SupportLevel,
    pub note: Option<&'static str>,
}

/// One row of the matrix. `per_agent` is in [`Agent::ALL`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    pub id: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub per_agent: [Support; 4],
}

impl Capability {
    pub fn support(&self, agent: Agent) -> Support {
        let at = Agent::ALL.iter().position(|a| *a == agent).unwrap_or(0);
        self.per_agent[at]
    }
}

const fn at(level: SupportLevel) -> Support {
    Support { level, note: None }
}

const fn noted(level: SupportLevel, note: &'static str) -> Support {
    Support {
        level,
        note: Some(note),
    }
}

const fn uniform(level: SupportLevel) -> [Support; 4] {
    [at(level), at(level), at(level), at(level)]
}

const fn claude_only(claude: Support) -> [Support; 4] {
    let planned = at(SupportLevel::Planned);
    [claude, planned, planned, planned]
}

/// The seeded matrix: committed surfaces plus backlog rows.
pub const CAPABILITIES: [Capability; 7] = [
    Capability {
        id: "session.view",
        group: "Sessions",
        title: "Session browser",
        description: "Browse sessions and open a transcript with full history.",
        per_agent: uniform(SupportLevel::Supported),
    },
    Capability {
        id: "session.debug-context",
        group: "Sessions",
        title: "Context debug",
        description: "Reproduce the context-budget forensics at peak.",
        per_agent: [
            at(SupportLevel::Supported),
            noted(
                SupportLevel::Partial,
                "Ground-truth usage + authoritative window; no on-disk memory attribution.",
            ),
            noted(
                SupportLevel::Partial,
                "Ground-truth usage; context window inferred from the model.",
            ),
            noted(
                SupportLevel::Partial,
                "Session-level ground-truth token totals; per-turn usage may be estimated and the context window is inferred from the model.",
            ),
        ],
    },
    Capability {
        id: "memory.view",
        group: "Memory",
        title: "Memory explorer",
        description: "View memories across all projects with budget + link graph.",
        per_agent: claude_only(at(SupportLevel::Supported)),
    },
    Capability {
        id: MEMORY_CRUD,
        group: "Memory",
        title: "Memory edit",
        description: "Create, edit and delete memories with safe atomic writes.",
        per_agent: claude_only(noted(
            SupportLevel::Supported,
            "Claude markdown memories only",
        )),
    },
    Capability {
        id: "mcp.dashboard",
        group: "MCP",
        title: "MCP dashboard",
        description: "Inspect configured MCP servers and their tools.",
        per_agent: uniform(SupportLevel::Planned),
    },
    Capability {
        id: "skills.browser",
        group: "Skills",
        title: "Skills browser",
        description: "Browse installed agent skills.",
        per_agent: uniform(SupportLevel::Planned),
    },
    Capability {
        id: "file-history.diff",
        group: "History",
        title: "File-history diff",
        description: "Diff file-history snapshots captured during a session.",
        per_agent: uniform(SupportLevel::Unsupported),
    },
];

/// Every capability row, in matrix order.
pub fn list_capabilities() -> &'static [Capability] {
    let _span = tracing::info_span!("CapabilityRegistry.list").entered();
    &CAPABILITIES
}

/// True when `agent` fully supports `capability_id`.
pub fn supports(capability_id: &str, agent: Agent) -> bool {
    let _span = tracing::info_span!(
        "CapabilityRegistry.supports",
        capabilityId = capability_id,
        agentId = agent.as_str()
    )
    .entered();
    CAPABILITIES
        .iter()
        .find(|c| c.id == capability_id)
        .is_some_and(|c| c.support(agent).level == SupportLevel::Supported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> &'static Capability {
        CAPABILITIES
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    }

    #[test]
    fn every_capability_covers_every_agent() {
        for c in &CAPABILITIES {
            assert_eq!(c.per_agent.len(), Agent::ALL.len(), "{}", c.id);
        }
    }

    #[test]
    fn seeds_the_committed_claude_surfaces_as_supported() {
        use SupportLevel::{Partial, Planned, Supported};
        let levels = |id| Agent::ALL.map(|a| find(id).support(a).level);
        for id in ["memory.view", MEMORY_CRUD] {
            assert_eq!(levels(id), [Supported, Planned, Planned, Planned]);
        }
        assert_eq!(levels("session.view"), [Supported; 4]);
        assert_eq!(
            levels("session.debug-context"),
            [Supported, Partial, Partial, Partial]
        );
    }

    #[test]
    fn supports_gates_writes_to_claude_only() {
        assert!(supports(MEMORY_CRUD, Agent::Claude));
        assert!(!supports(MEMORY_CRUD, Agent::Codex));
        assert!(!supports("unknown.cap", Agent::Claude));
    }
}
