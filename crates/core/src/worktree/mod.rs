//! Which project a session in a linked git worktree belongs to.
//!
//! A worktree's folder name says little (`t3-1b686728-...`, `workspace-1`),
//! so a session there is labelled with its main checkout instead. Tried in
//! order: what t3code recorded for the path, the worktree's `.git` file,
//! then the `<t3 home>/worktrees/<project>/<leaf>` layout. Nothing here
//! touches the network, and a missing dir is just no answer.

mod git;
#[cfg(test)]
mod tests;

use std::borrow::Cow;

pub use git::{Linked, linked, read_linked};

use crate::model::{Agent, SessionHeader, Worktree};
use crate::paths::{PathStyle, claude_slug, pi_slug};

/// What t3code recorded for one worktree path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub project: String,
    pub project_root: Option<String>,
    pub branch: Option<String>,
}

/// `path` without trailing separators; `/` stays.
pub fn normalize(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        &path[..path.len().min(1)]
    } else {
        trimmed
    }
}

/// A path's last segment.
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

/// `<home>/worktrees/<project>/<leaf>` (or a dir below it) as a worktree of
/// `<project>`, with no root or branch known.
pub fn under_t3_home(path: &str, home: &str) -> Option<Worktree> {
    let home = normalize(home);
    let base = PathStyle::NATIVE.join(&[home, "worktrees"]);
    let rest = path
        .strip_prefix(base.as_str())?
        .strip_prefix(['/', '\\'])?;
    let mut segs = rest.split(['/', '\\']);
    let (project, leaf) = (segs.next()?, segs.next()?);
    if project.is_empty() || leaf.is_empty() {
        return None;
    }
    Some(Worktree {
        project: project.to_owned(),
        project_root: None,
        path: PathStyle::NATIVE.join(&[&base, project, leaf]),
        branch: None,
    })
}

/// The worktree `cwd` is, when it is one. `known` answers for the paths
/// t3code recorded; `t3_home` is t3code's base dir (`~/.t3`).
pub fn resolve(
    cwd: &str,
    known: impl Fn(&str) -> Option<Known>,
    t3_home: Option<&str>,
) -> Option<Worktree> {
    let path = normalize(cwd);
    if path.is_empty() {
        return None;
    }
    if let Some(k) = known(path) {
        return Some(Worktree {
            project: k.project,
            project_root: k.project_root,
            path: path.to_owned(),
            branch: k.branch.or_else(|| linked(path).and_then(|l| l.branch)),
        });
    }
    if let Some(l) = linked(path) {
        return Some(Worktree {
            project: basename(&l.root).to_owned(),
            project_root: Some(l.root),
            path: path.to_owned(),
            branch: l.branch,
        });
    }
    t3_home.and_then(|home| under_t3_home(path, home))
}

/// The project `sessions ls` prints and `--project` matches for a row: for
/// a worktree row, what a row of its main checkout has (the slug for Claude
/// and Pi, the folder name for Codex and `OpenCode`); else the row's own.
pub fn row_project(h: &SessionHeader) -> Cow<'_, str> {
    let Some(w) = &h.worktree else {
        return Cow::Borrowed(h.project.as_str());
    };
    match (&w.project_root, h.agent) {
        (Some(root), Agent::Claude) => Cow::Owned(claude_slug(root)),
        (Some(root), Agent::Pi) => Cow::Owned(pi_slug(root)),
        (Some(root), Agent::Codex | Agent::Opencode) => Cow::Borrowed(basename(root)),
        (None, _) => Cow::Borrowed(w.project.as_str()),
    }
}
