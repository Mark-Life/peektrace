//! Memory through a source: every vault with the write gate's inputs, and
//! create, update and delete. Natively these run the core functions the
//! server runs, with its error mapping from `peektrace_core::wire`.

pub use peektrace_protocol::expected_mtime;
use peektrace_protocol::view::memory::{AllVaultsView, DeleteView, EntryView};
use peektrace_protocol::{Capability, MemoryCreateBody, MemoryFrontmatterPatch, MemoryUpdateBody};

/// What the explorer needs: every vault, the capability matrix and the
/// server mode.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryData {
    pub all: AllVaultsView,
    pub caps: Vec<Capability>,
    pub read_only: bool,
}

/// A memory write, addressed as the server's routes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryWrite {
    Create {
        project: String,
        body: MemoryCreateBody,
    },
    /// `expected_mtime`, in whole ms, turns on compare-and-swap.
    Update {
        project: String,
        name: String,
        frontmatter: Option<MemoryFrontmatterPatch>,
        body: Option<String>,
        expected_mtime: Option<i64>,
    },
    Delete {
        project: String,
        name: String,
    },
}

/// The body `PATCH .../entries/{name}` takes for an update.
#[expect(clippy::cast_precision_loss, reason = "file mtimes in ms fit f64")]
pub fn update_body(
    frontmatter: Option<&MemoryFrontmatterPatch>,
    body: Option<&str>,
    expected_mtime: Option<i64>,
) -> MemoryUpdateBody {
    MemoryUpdateBody {
        frontmatter: frontmatter.cloned(),
        body: body.map(str::to_owned),
        expected_mtime: expected_mtime.map(|m| m as f64),
    }
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
    use peektrace_core::memory::json::{all_vaults_json, delete_json, entry_json};
    use peektrace_core::memory::write::{
        FrontmatterPatch, MemoryCtx, MemoryError, MemoryUpdate, NewMemory, create, delete, update,
    };
    use peektrace_core::memory::{AllVaults, all_vaults};
    use peektrace_core::wire::{capability_rows, memory_api_error};
    use peektrace_core::{Agent, AgentRoots};
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

    /// The server's mapping of a write failure; an IO failure, which the
    /// server sends as a 500 defect, is a transport error here.
    fn memory_error(e: MemoryError, ctx: &MemoryCtx) -> LoadError {
        memory_api_error(e, &ctx.allowed_roots).map_or_else(
            |e| LoadError::Transport {
                status: None,
                message: e.to_string(),
            },
            LoadError::Api,
        )
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
                frontmatter,
                body,
                expected_mtime,
            } => {
                let span = memory_span!("app.memory.update");
                span.record("memory.project", project.as_str());
                span.record("memory.name", name.as_str());
                span.record("memory.cas", expected_mtime.is_some());
                let _entered = span.enter();
                let u = MemoryUpdate {
                    project,
                    name,
                    frontmatter: frontmatter.map(|f| FrontmatterPatch {
                        name: f.name,
                        description: f.description,
                        kind: f.kind,
                    }),
                    body,
                    expected_mtime,
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
}
