//! Capability matrix rules: columns, level order, labels, glosses and the
//! grouping of rows.

use peektrace_protocol::{AgentId, Capability, CapabilitySupport, SupportLevel};

/// Matrix columns, in order.
pub const AGENT_COLUMNS: [(AgentId, &str); 4] = [
    (AgentId::Claude, "Claude"),
    (AgentId::Codex, "Codex"),
    (AgentId::Pi, "Pi"),
    (AgentId::Opencode, "OpenCode"),
];

/// Legend order (`LEVEL_STYLES` keys).
pub const LEVELS: [SupportLevel; 4] = [
    SupportLevel::Supported,
    SupportLevel::Partial,
    SupportLevel::Planned,
    SupportLevel::Unsupported,
];

pub const fn level_name(l: SupportLevel) -> &'static str {
    match l {
        SupportLevel::Supported => "supported",
        SupportLevel::Partial => "partial",
        SupportLevel::Planned => "planned",
        SupportLevel::Unsupported => "unsupported",
    }
}

/// The pill text: the level, capitalised.
pub const fn level_label(l: SupportLevel) -> &'static str {
    match l {
        SupportLevel::Supported => "Supported",
        SupportLevel::Partial => "Partial",
        SupportLevel::Planned => "Planned",
        SupportLevel::Unsupported => "Unsupported",
    }
}

pub const fn level_gloss(l: SupportLevel) -> &'static str {
    match l {
        SupportLevel::Supported => "Built and working today.",
        SupportLevel::Partial => "Partially built — some paths work.",
        SupportLevel::Planned => "On the roadmap, not yet built.",
        SupportLevel::Unsupported => "Not applicable / no plan.",
    }
}

pub const fn support(cap: &Capability, agent: AgentId) -> &CapabilitySupport {
    match agent {
        AgentId::Claude => &cap.per_agent.claude,
        AgentId::Codex => &cap.per_agent.codex,
        AgentId::Pi => &cap.per_agent.pi,
        AgentId::Opencode => &cap.per_agent.opencode,
    }
}

/// Rows by `group`, groups in first-seen order.
pub fn group_by(caps: &[Capability]) -> Vec<(&str, Vec<&Capability>)> {
    let mut out: Vec<(&str, Vec<&Capability>)> = Vec::new();
    for cap in caps {
        match out.iter_mut().find(|(g, _)| *g == cap.group) {
            Some((_, rows)) => rows.push(cap),
            None => out.push((cap.group.as_str(), vec![cap])),
        }
    }
    out
}

/// A cell's note, else the gloss of its level.
pub fn cell_note(s: &CapabilitySupport) -> &str {
    s.note.as_deref().unwrap_or(level_gloss(s.level))
}

#[cfg(test)]
mod tests {
    use super::*;
    use peektrace_protocol::PerAgentSupport;

    fn cap(id: &str, group: &str) -> Capability {
        let s = CapabilitySupport {
            level: SupportLevel::Planned,
            note: None,
        };
        Capability {
            id: id.into(),
            group: group.into(),
            title: id.into(),
            description: String::new(),
            per_agent: PerAgentSupport {
                claude: s.clone(),
                codex: CapabilitySupport {
                    level: SupportLevel::Partial,
                    note: Some("only some".into()),
                },
                pi: s.clone(),
                opencode: s,
            },
        }
    }

    #[test]
    fn groups_keep_first_seen_order() {
        let caps = [cap("a", "B"), cap("b", "A"), cap("c", "B")];
        let groups: Vec<(&str, Vec<&str>)> = group_by(&caps)
            .into_iter()
            .map(|(g, rows)| (g, rows.iter().map(|c| c.id.as_str()).collect()))
            .collect();
        assert_eq!(groups, [("B", vec!["a", "c"]), ("A", vec!["b"])]);
    }

    #[test]
    fn notes_fall_back_to_the_gloss() {
        let c = cap("a", "G");
        assert_eq!(cell_note(support(&c, AgentId::Codex)), "only some");
        assert_eq!(
            cell_note(support(&c, AgentId::Pi)),
            "On the roadmap, not yet built."
        );
    }
}
