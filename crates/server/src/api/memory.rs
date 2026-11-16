//! Memory routes: read every vault, one project's vault, and Claude-only
//! create, update and delete with mtime compare-and-swap.

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::Method;
use peektrace_core::Agent;
use peektrace_core::memory::json::{
    all_vaults_json, delete_json, entry_json, projects_json, vault_json,
};
use peektrace_core::memory::write::{
    FrontmatterPatch, MemoryCtx, MemoryError, MemoryUpdate, NewMemory, create, delete, update,
};
use peektrace_core::memory::{all_vaults, get_vault, list_projects};
use peektrace_core::wire::memory_api_error;
use peektrace_protocol::{ApiError, READ_ONLY_REASON, expected_mtime};
use tracing::Instrument as _;

use super::params::{object_body, opt_num, opt_obj, opt_str, req_str, segment};
use super::{Failure, Res, Shared, blocking, finish, json};
use crate::AppState;

fn memory_error(e: MemoryError, ctx: &MemoryCtx) -> Failure {
    memory_api_error(e, &ctx.allowed_roots)
        .map_or_else(|e| Failure::Defect(e.to_string()), Failure::Api)
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
