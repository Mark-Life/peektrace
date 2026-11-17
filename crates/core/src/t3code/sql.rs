//! The queries t3code's databases answer.

// Thread queries yield `(provider, native id, title, branch, worktree,
// deleted, parent title)`; the parent title is `NULL` once the parent is
// deleted.

/// Legacy runtime rows with their legacy thread rows, for `state.sqlite`.
pub const LEGACY: &str = "SELECT r.provider_name,
        CASE r.provider_name WHEN 'codex' THEN json_extract(r.resume_cursor_json, '$.threadId')
            ELSE json_extract(r.resume_cursor_json, '$.resume') END,
        t.title, t.branch, t.worktree_path, t.deleted_at IS NOT NULL, NULL
     FROM provider_session_runtime r JOIN projection_threads t ON t.thread_id = r.thread_id
     WHERE json_valid(r.resume_cursor_json)";

/// The parent thread's title while it is not deleted.
const PARENT: &str = "LEFT JOIN orchestration_v2_projection_threads pt
        ON pt.thread_id = json_extract(t.payload_json, '$.lineage.parentThreadId')";

/// Legacy runtime rows named by the current thread rows, whose titles and
/// deletes stay fresh.
pub fn legacy_current() -> String {
    format!(
        "SELECT r.provider_name,
        CASE r.provider_name WHEN 'codex' THEN json_extract(r.resume_cursor_json, '$.threadId')
            ELSE json_extract(r.resume_cursor_json, '$.resume') END,
        t.title, json_extract(t.payload_json, '$.branch'),
        json_extract(t.payload_json, '$.worktreePath'), t.deleted_at IS NOT NULL,
        CASE WHEN pt.deleted_at IS NULL THEN pt.title END
     FROM provider_session_runtime r
     JOIN orchestration_v2_projection_threads t ON t.thread_id = r.thread_id
     {PARENT}
     WHERE json_valid(r.resume_cursor_json) AND json_valid(t.payload_json)"
    )
}

/// Current provider threads; the newest row for a native id wins.
pub fn current() -> String {
    format!(
        "SELECT json_extract(p.payload_json, '$.nativeThreadRef.driver'),
        json_extract(p.payload_json, '$.nativeThreadRef.nativeId'),
        t.title, json_extract(t.payload_json, '$.branch'),
        json_extract(t.payload_json, '$.worktreePath'), t.deleted_at IS NOT NULL,
        CASE WHEN pt.deleted_at IS NULL THEN pt.title END
     FROM orchestration_v2_projection_provider_threads p
     JOIN orchestration_v2_projection_threads t ON t.thread_id = p.thread_id
     {PARENT}
     WHERE json_valid(p.payload_json) AND json_valid(t.payload_json)
     ORDER BY p.updated_at"
    )
}

// Worktree queries yield `(worktree path, project title, project root,
// branch)`, live threads after deleted ones and newest last, so the last
// row for a path wins.

/// Legacy thread worktrees and their projects.
pub const LEGACY_WORKTREES: &str = "SELECT t.worktree_path, p.title, p.workspace_root, t.branch
     FROM projection_threads t JOIN projection_projects p ON p.project_id = t.project_id
     WHERE t.worktree_path IS NOT NULL
     ORDER BY t.deleted_at IS NULL, t.updated_at";

/// Current thread worktrees and their projects.
pub const WORKTREES: &str = "SELECT json_extract(t.payload_json, '$.worktreePath'), p.title,
        p.workspace_root, json_extract(t.payload_json, '$.branch')
     FROM orchestration_v2_projection_threads t
     JOIN projection_projects p ON p.project_id = t.project_id
     WHERE json_valid(t.payload_json) AND json_extract(t.payload_json, '$.worktreePath') IS NOT NULL
     ORDER BY t.deleted_at IS NULL, t.updated_at";
