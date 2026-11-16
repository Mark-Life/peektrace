//! The write gate (`lib/memory-support.ts`) and the create dialog's
//! project options (`routes/memory/project-options.ts`).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write as _;

use peektrace_js::text::{cmp_utf16, locale_compare};
use peektrace_protocol::view::SessionHeader;
use peektrace_protocol::view::memory::ProjectLabelView;
use peektrace_protocol::{AgentId, Capability, CapabilitySupport, SupportLevel};

use super::MEMORY_CRUD;
use crate::model::list::agent_label;

/// The `memory.crud` cell of each agent, in [`AgentId::ALL`] order; `None`
/// when the row or the cell is missing.
pub type CrudCells = [Option<CapabilitySupport>; 4];

pub fn crud_cells(caps: &[Capability]) -> CrudCells {
    caps.iter()
        .find(|c| c.id == MEMORY_CRUD)
        .map_or([None, None, None, None], |c| {
            let p = &c.per_agent;
            [&p.claude, &p.codex, &p.pi, &p.opencode].map(|s| Some(s.clone()))
        })
}

const fn level_of(cell: Option<&CapabilitySupport>) -> SupportLevel {
    match cell {
        Some(c) => c.level,
        None => SupportLevel::Unsupported,
    }
}

/// `Memory editing: Claude. Planned: Codex, Pi, OpenCode.`
pub fn support_summary(cells: &CrudCells) -> String {
    let names_at = |level: SupportLevel| {
        AgentId::ALL
            .iter()
            .zip(cells)
            .filter(|(_, c)| level_of(c.as_ref()) == level)
            .map(|(a, _)| agent_label(*a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let supported = names_at(SupportLevel::Supported);
    let others: String = [
        (SupportLevel::Partial, "Partial"),
        (SupportLevel::Planned, "Planned"),
        (SupportLevel::Unsupported, "Not supported"),
    ]
    .into_iter()
    .map(|(level, label)| (label, names_at(level)))
    .filter(|(_, names)| !names.is_empty())
    .fold(String::new(), |mut out, (label, names)| {
        let _ = write!(out, " {label}: {names}.");
        out
    });
    let who = if supported.is_empty() {
        "no agent yet"
    } else {
        supported.as_str()
    };
    format!("Memory editing: {who}.{others}")
}

/// What stops memory writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    ReadOnly,
    Capability,
}

impl Blocked {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Capability => "capability",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub level: SupportLevel,
    pub note: Option<String>,
    pub blocked_by: Option<Blocked>,
}

impl Verdict {
    pub const fn can_write(&self) -> bool {
        self.blocked_by.is_none()
    }
}

/// A read-only server blocks first; else Claude's cell decides.
pub fn crud_verdict(cells: &CrudCells, read_only: bool) -> Verdict {
    let claude = cells[0].as_ref();
    let level = level_of(claude);
    let blocked_by = if read_only {
        Some(Blocked::ReadOnly)
    } else if level == SupportLevel::Supported {
        None
    } else {
        Some(Blocked::Capability)
    };
    Verdict {
        level,
        note: claude.and_then(|c| c.note.clone()),
        blocked_by,
    }
}

pub const fn level_str(level: SupportLevel) -> &'static str {
    match level {
        SupportLevel::Supported => "supported",
        SupportLevel::Partial => "partial",
        SupportLevel::Planned => "planned",
        SupportLevel::Unsupported => "unsupported",
    }
}

/// The banner title for a capability block.
pub fn capability_title(level: SupportLevel) -> String {
    let word = match level {
        SupportLevel::Unsupported => "not supported",
        other => level_str(other),
    };
    format!("Memory editing is {word}")
}

/// One project a memory can go into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOption {
    pub slug: String,
    pub label: String,
    /// Newest Claude session `updatedAt`, absent for vault-only projects.
    pub last_active: Option<String>,
}

/// Claude projects from session headers (newest first) and the server's
/// labels; undated projects last, ties by label. An empty `updatedAt`
/// stands for a missing one.
pub fn project_options(
    headers: &[SessionHeader],
    labels: &[ProjectLabelView],
) -> Vec<ProjectOption> {
    let mut active: Vec<(&str, Option<&str>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for h in headers.iter().filter(|h| h.agent == AgentId::Claude) {
        let date = Some(h.updated_at.as_str()).filter(|s| !s.is_empty());
        match at.get(h.project.as_str()) {
            None => {
                at.insert(&h.project, active.len());
                active.push((&h.project, date));
            }
            Some(&i) => {
                let prev = active[i].1;
                let newer = |d: &str| prev.is_none_or(|p| cmp_utf16(d, p) == Ordering::Greater);
                if prev.is_none() || date.is_some_and(newer) {
                    active[i].1 = date;
                }
            }
        }
    }
    let mut label: HashMap<&str, &str> = HashMap::new();
    let mut slugs: Vec<&str> = active.iter().map(|(s, _)| *s).collect();
    for l in labels {
        if label.insert(&l.slug, &l.project).is_none() && !at.contains_key(l.slug.as_str()) {
            slugs.push(&l.slug);
        }
    }
    let mut out: Vec<ProjectOption> = slugs
        .into_iter()
        .map(|slug| ProjectOption {
            slug: slug.to_owned(),
            label: label.get(slug).copied().unwrap_or(slug).to_owned(),
            last_active: at.get(slug).and_then(|&i| active[i].1).map(str::to_owned),
        })
        .collect();
    out.sort_by(by_recency);
    out
}

fn by_recency(a: &ProjectOption, b: &ProjectOption) -> Ordering {
    match (&a.last_active, &b.last_active) {
        (x, y) if x == y => locale_compare(&a.label, &b.label),
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(x), Some(y)) => {
            if cmp_utf16(x, y) == Ordering::Greater {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
    }
}
