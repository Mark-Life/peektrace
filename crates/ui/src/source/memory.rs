//! Memory through a source: every vault with the write gate's inputs, and
//! create, update and delete. Natively these run the core functions the
//! server runs, with its error mapping, so both sources answer alike.

use peektrace_protocol::view::memory::{AllVaultsView, DeleteView, EntryView};
use peektrace_protocol::{Capability, MemoryCreateBody, MemoryUpdateBody};

/// What the explorer needs: every vault, the capability matrix and the
/// server mode.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryData {
    pub all: AllVaultsView,
    pub caps: Vec<Capability>,
    pub read_only: bool,
}

/// A memory write, addressed as the server's routes are.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryWrite {
    Create {
        project: String,
        body: MemoryCreateBody,
    },
    Update {
        project: String,
        name: String,
        body: MemoryUpdateBody,
    },
    Delete {
        project: String,
        name: String,
    },
}

/// What a write answers.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryDone {
    Entry(EntryView),
    Deleted(DeleteView),
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use peektrace_core::capabilities::{self, MEMORY_CRUD, SupportLevel as CoreLevel};
    use peektrace_core::memory::fs::{ChangeReason, WriteError};
    use peektrace_core::memory::json::{all_vaults_json, delete_json, entry_json};
    use peektrace_core::memory::write::{
        FrontmatterPatch, MemoryCtx, MemoryError, MemoryUpdate, NewMemory, create, delete, update,
    };
    use peektrace_core::memory::{AllVaults, all_vaults};
    use peektrace_core::{Agent, AgentRoots};
    use peektrace_protocol::{
        AgentId, ApiError, Capability, CapabilitySupport, FileChangeReason, PerAgentSupport,
        SupportLevel,
    };
    use tracing::{field, info_span};

    use super::{MemoryData, MemoryDone, MemoryWrite};
    use crate::source::{LoadError, decode};

    macro_rules! memory_span {
        ($name:literal) => {
            info_span!(
                parent: None,
                $name,
                peektrace.root = true,
                peektrace.kind = "app",
                memory.vaults = field::Empty,
                memory.projects = field::Empty,
                memory.project = field::Empty,
                memory.name = field::Empty,
                memory.cas = field::Empty,
                memory.dangling = field::Empty,
                error = field::Empty
            )
        };
    }

    const fn agent_id(a: Agent) -> AgentId {
        match a {
            Agent::Claude => AgentId::Claude,
            Agent::Codex => AgentId::Codex,
            Agent::Pi => AgentId::Pi,
            Agent::Opencode => AgentId::Opencode,
        }
    }

    const fn level(l: CoreLevel) -> SupportLevel {
        match l {
            CoreLevel::Supported => SupportLevel::Supported,
            CoreLevel::Partial => SupportLevel::Partial,
            CoreLevel::Planned => SupportLevel::Planned,
            CoreLevel::Unsupported => SupportLevel::Unsupported,
        }
    }

    /// The matrix as `GET /api/capabilities` sends it.
    pub fn capability_rows() -> Vec<Capability> {
        capabilities::list_capabilities()
            .iter()
            .map(|c| {
                let [claude, codex, pi, opencode] = c.per_agent.map(|s| CapabilitySupport {
                    level: level(s.level),
                    note: s.note.map(str::to_owned),
                });
                Capability {
                    id: c.id.to_owned(),
                    group: c.group.to_owned(),
                    title: c.title.to_owned(),
                    description: c.description.to_owned(),
                    per_agent: PerAgentSupport {
                        claude,
                        codex,
                        pi,
                        opencode,
                    },
                }
            })
            .collect()
    }

    /// The server's mapping of a write failure; an IO failure, which the
    /// server sends as a 500 defect, is a transport error here.
    pub fn memory_error(e: MemoryError, ctx: &MemoryCtx) -> LoadError {
        let api = match e {
            MemoryError::CapabilityUnsupported { agent } => ApiError::CapabilityUnsupportedError {
                capability_id: MEMORY_CRUD.to_owned(),
                agent_id: agent_id(agent),
            },
            MemoryError::Validation { reason, name } => ApiError::MemoryValidationError {
                reason,
                name: Some(name),
            },
            MemoryError::NotFound { project, name } => {
                ApiError::MemoryNotFoundError { project, name }
            }
            MemoryError::Write(WriteError::Changed { path, reason }) => {
                ApiError::FileChangedError {
                    path,
                    reason: match reason {
                        ChangeReason::Mtime => FileChangeReason::Mtime,
                        ChangeReason::Missing => FileChangeReason::Missing,
                    },
                }
            }
            MemoryError::Write(WriteError::OutsideRoots { path }) => {
                ApiError::PathOutsideRootError {
                    path,
                    roots: ctx.allowed_roots.clone(),
                }
            }
            MemoryError::Write(e @ WriteError::Io { .. }) => {
                return LoadError::Transport {
                    status: None,
                    message: e.to_string(),
                };
            }
        };
        LoadError::Api(api)
    }

    /// `expectedMtime` as the whole ms core compares; any other number can
    /// never match, so it maps to a value no file has.
    #[expect(clippy::cast_possible_truncation, reason = "checked to be in range")]
    pub fn expected_mtime(n: f64) -> i64 {
        #[expect(clippy::cast_precision_loss, reason = "range bounds")]
        let in_range = n.fract() == 0.0 && n >= i64::MIN as f64 && n < i64::MAX as f64;
        if in_range { n as i64 } else { i64::MIN }
    }

    fn record_error<T>(span: &tracing::Span, out: &Result<T, LoadError>) {
        if let Err(e) = out {
            span.record("error", field::display(e));
        }
    }

    /// Every Claude vault, the matrix, and never read-only (no server).
    pub fn load(roots: &AgentRoots) -> Result<MemoryData, LoadError> {
        let span = memory_span!("app.memory.all");
        let _entered = span.enter();
        let all: AllVaults = all_vaults(&roots.claude);
        span.record("memory.vaults", all.vaults.len());
        span.record("memory.projects", all.projects.len());
        let out = decode(&all_vaults_json(&all, false)).map(|all| MemoryData {
            all,
            caps: capability_rows(),
            read_only: false,
        });
        record_error(&span, &out);
        out
    }

    /// One write with the app's roots and the system temp dir.
    pub fn write(
        roots: &AgentRoots,
        tmp_dir: &str,
        op: MemoryWrite,
    ) -> Result<MemoryDone, LoadError> {
        let ctx = MemoryCtx::new(roots, tmp_dir);
        let fail = |e| memory_error(e, &ctx);
        match op {
            MemoryWrite::Create { project, body } => {
                let span = memory_span!("app.memory.create");
                span.record("memory.project", project.as_str());
                span.record("memory.name", body.name.as_str());
                let _entered = span.enter();
                let m = NewMemory {
                    project,
                    name: body.name,
                    description: body.description,
                    kind: body.kind,
                    body: body.body,
                };
                let out = create(&ctx, Agent::Claude, &m)
                    .map_err(fail)
                    .and_then(|e| decode(&entry_json(&e, false)).map(MemoryDone::Entry));
                record_error(&span, &out);
                out
            }
            MemoryWrite::Update {
                project,
                name,
                body,
            } => {
                let span = memory_span!("app.memory.update");
                span.record("memory.project", project.as_str());
                span.record("memory.name", name.as_str());
                span.record("memory.cas", body.expected_mtime.is_some());
                let _entered = span.enter();
                let u = MemoryUpdate {
                    project,
                    name,
                    frontmatter: body.frontmatter.map(|f| FrontmatterPatch {
                        name: f.name,
                        description: f.description,
                        kind: f.kind,
                    }),
                    body: body.body,
                    expected_mtime: body.expected_mtime.map(expected_mtime),
                };
                let out = update(&ctx, Agent::Claude, &u)
                    .map_err(fail)
                    .and_then(|e| decode(&entry_json(&e, false)).map(MemoryDone::Entry));
                record_error(&span, &out);
                out
            }
            MemoryWrite::Delete { project, name } => {
                let span = memory_span!("app.memory.delete");
                span.record("memory.project", project.as_str());
                span.record("memory.name", name.as_str());
                let _entered = span.enter();
                let out = delete(&ctx, Agent::Claude, &project, &name)
                    .map_err(fail)
                    .and_then(|r| {
                        span.record("memory.dangling", r.dangling.len());
                        decode(&delete_json(&r, false)).map(MemoryDone::Deleted)
                    });
                record_error(&span, &out);
                out
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use peektrace_core::memory::fs::{ChangeReason, WriteError};

        use super::*;

        fn ctx() -> MemoryCtx {
            MemoryCtx {
                claude: Vec::new(),
                allowed_roots: vec!["/r".to_owned()],
            }
        }

        #[test]
        fn errors_map_as_the_server_maps_them() {
            let api = |e| match memory_error(e, &ctx()) {
                LoadError::Api(a) => Some(a),
                _ => None,
            };
            assert_eq!(
                api(MemoryError::CapabilityUnsupported {
                    agent: Agent::Codex
                }),
                Some(ApiError::CapabilityUnsupportedError {
                    capability_id: MEMORY_CRUD.to_owned(),
                    agent_id: AgentId::Codex,
                })
            );
            assert_eq!(
                api(MemoryError::Validation {
                    reason: "r".into(),
                    name: "n".into()
                }),
                Some(ApiError::MemoryValidationError {
                    reason: "r".into(),
                    name: Some("n".into())
                })
            );
            assert_eq!(
                api(MemoryError::NotFound {
                    project: "p".into(),
                    name: "n".into()
                }),
                Some(ApiError::MemoryNotFoundError {
                    project: "p".into(),
                    name: "n".into()
                })
            );
            assert_eq!(
                api(MemoryError::Write(WriteError::Changed {
                    path: "/f".into(),
                    reason: ChangeReason::Missing
                })),
                Some(ApiError::FileChangedError {
                    path: "/f".into(),
                    reason: FileChangeReason::Missing
                })
            );
            assert_eq!(
                api(MemoryError::Write(WriteError::OutsideRoots {
                    path: "/x".into()
                })),
                Some(ApiError::PathOutsideRootError {
                    path: "/x".into(),
                    roots: vec!["/r".into()]
                })
            );
            assert_eq!(expected_mtime(1_700_000_000_123.0), 1_700_000_000_123);
            assert_eq!(expected_mtime(1.5), i64::MIN);
        }
    }
}
