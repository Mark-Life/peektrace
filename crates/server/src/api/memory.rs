//! Memory routes: read every vault, one project's vault, and Claude-only
//! create, update and delete with mtime compare-and-swap.

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::Method;
use peektrace_core::Agent;
use peektrace_core::capabilities::MEMORY_CRUD;
use peektrace_core::memory::fs::{ChangeReason, WriteError};
use peektrace_core::memory::json::{
    all_vaults_json, delete_json, entry_json, projects_json, vault_json,
};
use peektrace_core::memory::write::{
    FrontmatterPatch, MemoryCtx, MemoryError, MemoryUpdate, NewMemory, create, delete, update,
};
use peektrace_core::memory::{all_vaults, get_vault, list_projects};
use peektrace_protocol::{ApiError, FileChangeReason, READ_ONLY_REASON};
use tracing::Instrument as _;

use super::params::{object_body, opt_num, opt_obj, opt_str, req_str, segment};
use super::{Failure, Res, Shared, agent_id, blocking, finish, json};
use crate::AppState;

fn memory_error(e: MemoryError, ctx: &MemoryCtx) -> Failure {
    let api = match e {
        MemoryError::CapabilityUnsupported { agent } => ApiError::CapabilityUnsupportedError {
            capability_id: MEMORY_CRUD.to_owned(),
            agent_id: agent_id(agent),
        },
        MemoryError::Validation { reason, name } => ApiError::MemoryValidationError {
            reason,
            name: Some(name),
        },
        MemoryError::NotFound { project, name } => ApiError::MemoryNotFoundError { project, name },
        MemoryError::Write(WriteError::Changed { path, reason }) => ApiError::FileChangedError {
            path,
            reason: match reason {
                ChangeReason::Mtime => FileChangeReason::Mtime,
                ChangeReason::Missing => FileChangeReason::Missing,
            },
        },
        MemoryError::Write(WriteError::OutsideRoots { path }) => ApiError::PathOutsideRootError {
            path,
            roots: ctx.allowed_roots.clone(),
        },
        MemoryError::Write(e @ WriteError::Io { .. }) => return Failure::Defect(e.to_string()),
    };
    Failure::Api(api)
}

/// Writes are refused in read-only mode before any disk access.
fn writable(state: &AppState) -> Result<(), ApiError> {
    if state.read_only {
        return Err(ApiError::MemoryValidationError {
            reason: READ_ONLY_REASON.to_owned(),
            name: None,
        });
    }
    Ok(())
}

/// Run a memory write with the server's roots, mapping its errors.
async fn write<T: Send + 'static>(
    state: std::sync::Arc<AppState>,
    f: impl FnOnce(&MemoryCtx) -> Result<T, MemoryError> + Send + 'static,
) -> Result<T, Failure> {
    let current = tracing::Span::current();
    blocking(move || {
        let _entered = current.enter();
        let ctx = MemoryCtx::new(&state.roots, &state.tmp_dir);
        f(&ctx).map_err(|e| memory_error(e, &ctx))
    })
    .await
}

/// A read: core work off the runtime, answered as JSON.
async fn read(
    state: std::sync::Arc<AppState>,
    f: impl FnOnce(&AppState) -> String + Send + 'static,
) -> Result<Res, Failure> {
    let current = tracing::Span::current();
    let body = blocking(move || {
        let _entered = current.enter();
        Ok::<_, ApiError>(f(&state))
    })
    .await?;
    Ok(json(body))
}

pub async fn memory_all(State(state): Shared, method: Method) -> Res {
    let span = rpc_span!("memory.allVaults", method, "/api/memory");
    async move {
        finish(
            read(state, |s| {
                all_vaults_json(&all_vaults(&s.roots.claude), false)
            })
            .await,
        )
    }
    .instrument(span)
    .await
}

pub async fn memory_projects(State(state): Shared, method: Method) -> Res {
    let span = rpc_span!("memory.projects", method, "/api/memory/projects");
    async move {
        finish(
            read(state, |s| {
                projects_json(&list_projects(&s.roots.claude), false)
            })
            .await,
        )
    }
    .instrument(span)
    .await
}

pub async fn memory_vault(
    State(state): Shared,
    method: Method,
    path: Result<Path<String>, PathRejection>,
) -> Res {
    let span = rpc_span!("memory.vault", method, "/api/memory/projects/{project}");
    async move {
        let result = async {
            let project = segment(path)?;
            read(state, move |s| {
                vault_json(&get_vault(&s.roots.claude, &project), false)
            })
            .await
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn memory_create(
    State(state): Shared,
    method: Method,
    path: Result<Path<String>, PathRejection>,
    body: Bytes,
) -> Res {
    let span = rpc_span!(
        "memory.create",
        method,
        "/api/memory/projects/{project}/entries"
    );
    async move {
        let result = async {
            let project = segment(path)?;
            let o = object_body(&body, false)?;
            let m = NewMemory {
                project,
                name: req_str(&o, "name")?,
                description: req_str(&o, "description")?,
                kind: req_str(&o, "type")?,
                body: req_str(&o, "body")?,
            };
            writable(&state)?;
            let entry = write(state, move |ctx| create(ctx, Agent::Claude, &m)).await?;
            Ok(json(entry_json(&entry, false)))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

/// `expectedMtime` as the whole ms core compares; any other number can
/// never match, so it maps to a value no file has.
#[expect(clippy::cast_possible_truncation, reason = "checked to be in range")]
fn expected_mtime(n: f64) -> i64 {
    #[expect(clippy::cast_precision_loss, reason = "range bounds")]
    let in_range = n.fract() == 0.0 && n >= i64::MIN as f64 && n < i64::MAX as f64;
    if in_range { n as i64 } else { i64::MIN }
}

fn patch_of(o: &peektrace_core::js::value::JsObject) -> Result<FrontmatterPatch, ApiError> {
    Ok(FrontmatterPatch {
        name: opt_str(o, "name")?,
        description: opt_str(o, "description")?,
        kind: opt_str(o, "type")?,
    })
}

pub async fn memory_update(
    State(state): Shared,
    method: Method,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Bytes,
) -> Res {
    let span = rpc_span!(
        "memory.update",
        method,
        "/api/memory/projects/{project}/entries/{name}"
    );
    async move {
        let result = async {
            let (project, name) = segment(path)?;
            let o = object_body(&body, false)?;
            let u = MemoryUpdate {
                project,
                name,
                frontmatter: opt_obj(&o, "frontmatter")?.map(patch_of).transpose()?,
                body: opt_str(&o, "body")?,
                expected_mtime: opt_num(&o, "expectedMtime")?.map(expected_mtime),
            };
            writable(&state)?;
            let entry = write(state, move |ctx| update(ctx, Agent::Claude, &u)).await?;
            Ok(json(entry_json(&entry, false)))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

pub async fn memory_delete(
    State(state): Shared,
    method: Method,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Res {
    let span = rpc_span!(
        "memory.delete",
        method,
        "/api/memory/projects/{project}/entries/{name}"
    );
    async move {
        let result = async {
            let (project, name) = segment(path)?;
            writable(&state)?;
            let done = write(state, move |ctx| {
                delete(ctx, Agent::Claude, &project, &name)
            })
            .await?;
            Ok(json(delete_json(&done, false)))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whole_ms_can_match() {
        assert_eq!(expected_mtime(1_700_000_000_123.0), 1_700_000_000_123);
        assert_eq!(expected_mtime(1.5), i64::MIN);
        assert_eq!(expected_mtime(1e300), i64::MIN);
    }
}
