//! The settings editor's row model and the transforms between it and the
//! stored document. Pure.

use peektrace_core::Agent;
use peektrace_core::js::text;
use peektrace_core::settings_doc::{DocEntry, SettingsDoc};

/// One editable root row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRow {
    pub id: u64,
    pub path: String,
    pub label: String,
}

/// Rows per agent, in [`Agent::ALL`] order.
pub type Rows = [Vec<EditRow>; 4];

pub fn agent_at(a: Agent) -> usize {
    Agent::ALL.iter().position(|x| *x == a).unwrap_or(0)
}

/// A navigable line: a root row, or an agent's `+ add root`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Root { agent: Agent, id: u64 },
    Add { agent: Agent },
}

/// Seed rows from a stored document; `next` mints row ids.
pub fn seed_rows(doc: &SettingsDoc, next: &mut u64) -> Rows {
    let mut rows: Rows = Default::default();
    for (agent, entries) in doc.roots.iter().flatten() {
        rows[agent_at(*agent)] = entries
            .iter()
            .map(|e| {
                *next += 1;
                EditRow {
                    id: *next,
                    path: e.path.clone(),
                    label: e.label.clone().unwrap_or_default(),
                }
            })
            .collect();
    }
    rows
}

/// Trim, drop empty paths and empty labels, omit agents with no rows.
pub fn assemble(rows: &Rows) -> SettingsDoc {
    let roots: Vec<(Agent, Vec<DocEntry>)> = Agent::ALL
        .iter()
        .zip(rows)
        .filter_map(|(agent, list)| {
            let entries: Vec<DocEntry> = list
                .iter()
                .map(|r| (text::trim(&r.path), text::trim(&r.label)))
                .filter(|(p, _)| !p.is_empty())
                .map(|(p, l)| DocEntry {
                    path: p.to_owned(),
                    label: (!l.is_empty()).then(|| l.to_owned()),
                })
                .collect();
            (!entries.is_empty()).then_some((*agent, entries))
        })
        .collect();
    SettingsDoc {
        roots: (!roots.is_empty()).then_some(roots),
    }
}

pub fn build_entries(rows: &Rows) -> Vec<Nav> {
    Agent::ALL
        .iter()
        .zip(rows)
        .flat_map(|(agent, list)| {
            list.iter()
                .map(|r| Nav::Root {
                    agent: *agent,
                    id: r.id,
                })
                .chain(std::iter::once(Nav::Add { agent: *agent }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_trims_and_drops_empty() {
        let mut rows: Rows = Default::default();
        rows[0] = vec![
            EditRow {
                id: 1,
                path: " ~/a ".into(),
                label: " ".into(),
            },
            EditRow {
                id: 2,
                path: String::new(),
                label: "x".into(),
            },
        ];
        rows[1] = vec![EditRow {
            id: 3,
            path: "~/c".into(),
            label: "work".into(),
        }];
        let doc = assemble(&rows);
        assert_eq!(
            doc.roots,
            Some(vec![
                (
                    Agent::Claude,
                    vec![DocEntry {
                        path: "~/a".into(),
                        label: None
                    }]
                ),
                (
                    Agent::Codex,
                    vec![DocEntry {
                        path: "~/c".into(),
                        label: Some("work".into())
                    }]
                ),
            ])
        );
        assert_eq!(assemble(&Rows::default()).roots, None);
    }

    #[test]
    fn seed_then_entries() {
        let doc = SettingsDoc {
            roots: Some(vec![(
                Agent::Pi,
                vec![DocEntry {
                    path: "p".into(),
                    label: Some("l".into()),
                }],
            )]),
        };
        let mut next = 0;
        let rows = seed_rows(&doc, &mut next);
        assert_eq!(rows[2][0].label, "l");
        let nav = build_entries(&rows);
        assert_eq!(nav.len(), 5);
        assert_eq!(
            nav[2],
            Nav::Root {
                agent: Agent::Pi,
                id: 1
            }
        );
        assert_eq!(assemble(&rows), doc);
    }
}
