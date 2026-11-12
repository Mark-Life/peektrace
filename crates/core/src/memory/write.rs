//! Memory create, update and delete. Every write is Claude-only (the
//! `memory.crud` capability), goes through an atomic write confined to the
//! agent roots, and keeps the MEMORY.md pointer lines in step.

use tracing::info_span;

use super::frontmatter::{compose_file, parse_frontmatter, serialize_block};
use super::fs::{WriteError, atomic_write, exists, is_within, read_text, stat_mtime};
use super::index_edit::{index_line_for, insert_index_line, remove_index_line, update_index_line};
use super::parse::build_entry;
use super::read::{INDEX_FILE, build_vault, memory_dir};
use super::types::{
    DanglingRef, DeleteResult, EdgeKind, Field, Frontmatter, MEMORY_TYPES, MemoryEntry, Shape,
};
use crate::capabilities::{MEMORY_CRUD, supports};
use crate::listing::AgentRoots;
use crate::model::Agent;
use crate::paths::PathStyle;
use crate::roots::Source;

/// What memory writes may touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCtx {
    pub claude: Vec<Source>,
    /// Every agent root (config dir and projects dir) plus the temp dir.
    pub allowed_roots: Vec<String>,
}

impl MemoryCtx {
    pub fn new(roots: &AgentRoots, tmp_dir: &str) -> Self {
        let allowed_roots = Agent::ALL
            .iter()
            .flat_map(|a| roots.of(*a))
            .flat_map(|s| [s.home.clone(), s.projects_root.clone()])
            .chain(std::iter::once(tmp_dir.to_owned()))
            .collect();
        Self {
            claude: roots.claude.clone(),
            allowed_roots,
        }
    }
}

/// A failed memory write, worded as the CLI prints it.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Unsupported for agent \"{agent}\": {MEMORY_CRUD}")]
    CapabilityUnsupported { agent: Agent },
    #[error("Invalid memory \"{name}\": {reason}")]
    Validation { reason: String, name: String },
    #[error("Memory \"{name}\" not found in project \"{project}\".")]
    NotFound { project: String, name: String },
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// Frontmatter fields an update may change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontmatterPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
}

fn join(parts: &[&str]) -> String {
    PathStyle::NATIVE.join(parts)
}

fn ensure_capable(agent: Agent) -> Result<(), MemoryError> {
    if supports(MEMORY_CRUD, agent) {
        Ok(())
    } else {
        Err(MemoryError::CapabilityUnsupported { agent })
    }
}

fn invalid(reason: &str, name: &str) -> MemoryError {
    MemoryError::Validation {
        reason: reason.to_owned(),
        name: name.to_owned(),
    }
}

fn check_type(kind: &str, name: &str) -> Result<(), MemoryError> {
    if MEMORY_TYPES.contains(&kind) {
        Ok(())
    } else {
        Err(invalid(
            &format!("type must be one of {}", MEMORY_TYPES.join("/")),
            name,
        ))
    }
}

/// `^[a-z0-9]+(?:-[a-z0-9]+)*$`.
fn is_kebab(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// One path segment: not empty, `.` or `..`, and no separator.
fn is_plain_slug(slug: &str) -> bool {
    !slug.is_empty() && slug != "." && slug != ".." && PathStyle::NATIVE.basename(slug) == slug
}

/// Rewrite MEMORY.md through `transform`, with a compare-and-swap on its
/// mtime, or write `fallback` when it does not exist.
fn write_index(
    ctx: &MemoryCtx,
    path: &str,
    transform: impl FnOnce(&str) -> String,
    fallback: &str,
) -> Result<(), MemoryError> {
    if exists(path) {
        let raw = read_text(path).map_err(WriteError::from)?;
        let mtime = stat_mtime(path).map_err(WriteError::from)?;
        atomic_write(path, &transform(&raw), Some(mtime), &ctx.allowed_roots)?;
    } else {
        atomic_write(path, fallback, None, &ctx.allowed_roots)?;
    }
    Ok(())
}

fn stat_entry(path: &str, content: &str) -> Result<MemoryEntry, MemoryError> {
    let mtime = stat_mtime(path).map_err(WriteError::from)?;
    Ok(build_entry(path, content, mtime, true))
}

/// What a new memory holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMemory {
    pub project: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub body: String,
}

/// Create a memory file and its MEMORY.md pointer line.
pub fn create(ctx: &MemoryCtx, agent: Agent, m: &NewMemory) -> Result<MemoryEntry, MemoryError> {
    let _span = info_span!("Memory.create", name = m.name.as_str()).entered();
    ensure_capable(agent)?;
    if !is_kebab(&m.name) {
        return Err(invalid("name must be kebab-case", &m.name));
    }
    check_type(&m.kind, &m.name)?;
    let dir = memory_dir(&ctx.claude, &m.project);
    let project_dir = std::path::Path::new(&dir)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !(is_plain_slug(&m.project) && exists(&project_dir)) {
        return Err(invalid("unknown project", &m.name));
    }
    std::fs::create_dir_all(&dir).map_err(WriteError::from)?;
    let file_name = format!("{}.md", m.name);
    let path = join(&[&dir, &file_name]);
    if exists(&path) {
        return Err(invalid("a memory with this name already exists", &m.name));
    }
    let mut fm = Frontmatter {
        shape: Shape::Flat,
        ..Frontmatter::default()
    };
    fm.set(Field::Name, &m.name);
    fm.set(Field::Description, &m.description);
    fm.set(Field::Type, &m.kind);
    fm.raw = serialize_block(&fm);
    let content = compose_file(&fm, &m.body);
    atomic_write(&path, &content, None, &ctx.allowed_roots)?;
    let line = index_line_for(&m.name, &file_name, Some(&m.description));
    write_index(
        ctx,
        &join(&[&dir, INDEX_FILE]),
        |raw| insert_index_line(raw, &line),
        &format!("{line}\n"),
    )?;
    stat_entry(&path, &content)
}

/// A change to one memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryUpdate {
    pub project: String,
    pub name: String,
    pub frontmatter: Option<FrontmatterPatch>,
    pub body: Option<String>,
    pub expected_mtime: Option<i64>,
}

/// Apply a patch; any patch re-emits the block canonically.
fn apply_patch(current: &Frontmatter, patch: Option<&FrontmatterPatch>) -> Frontmatter {
    let mut fm = current.clone();
    let Some(p) = patch else {
        return fm;
    };
    for (f, v) in [
        (Field::Name, &p.name),
        (Field::Description, &p.description),
        (Field::Type, &p.kind),
    ] {
        if let Some(v) = v {
            fm.set(f, v);
        }
    }
    fm.raw = serialize_block(&fm);
    fm
}

/// Edit a memory's body or frontmatter, optionally only when its mtime is
/// still `expected_mtime`.
pub fn update(ctx: &MemoryCtx, agent: Agent, u: &MemoryUpdate) -> Result<MemoryEntry, MemoryError> {
    let _span = info_span!("Memory.update", name = u.name.as_str()).entered();
    ensure_capable(agent)?;
    let patch = u.frontmatter.as_ref();
    if let Some(kind) = patch.and_then(|p| p.kind.as_deref()) {
        check_type(kind, &u.name)?;
    }
    let dir = memory_dir(&ctx.claude, &u.project);
    let file_name = format!("{}.md", u.name);
    let file_path = join(&[&dir, &file_name]);
    let text = read_text(&file_path).map_err(|_| MemoryError::NotFound {
        project: u.project.clone(),
        name: u.name.clone(),
    })?;
    let (current, current_body) = parse_frontmatter(&text);
    let fm = apply_patch(&current, patch);
    let content = compose_file(&fm, u.body.as_deref().unwrap_or(&current_body));
    atomic_write(&file_path, &content, u.expected_mtime, &ctx.allowed_roots)?;
    if patch.is_some_and(|p| p.name.is_some() || p.description.is_some()) {
        let label = fm.get(Field::Name).unwrap_or(&u.name);
        let line = index_line_for(label, &file_name, fm.get(Field::Description));
        write_index(
            ctx,
            &join(&[&dir, INDEX_FILE]),
            |raw| update_index_line(raw, &u.name, &line),
            &format!("{line}\n"),
        )?;
    }
    stat_entry(&file_path, &content)
}

/// Delete a memory and its pointer lines; report the body links it leaves
/// dangling. MEMORY.md is written even when nothing changed, and created
/// empty when missing.
pub fn delete(
    ctx: &MemoryCtx,
    agent: Agent,
    project: &str,
    name: &str,
) -> Result<DeleteResult, MemoryError> {
    let _span = info_span!("Memory.delete", name).entered();
    ensure_capable(agent)?;
    let dir = memory_dir(&ctx.claude, project);
    let path = join(&[&dir, &format!("{name}.md")]);
    let vault = build_vault(&ctx.claude, project, project);
    if !vault.entries.iter().any(|e| e.slug == name) {
        return Err(MemoryError::NotFound {
            project: project.to_owned(),
            name: name.to_owned(),
        });
    }
    let dangling = vault
        .graph
        .edges
        .iter()
        .filter(|e| {
            e.kind != EdgeKind::Index && e.resolved_to.as_deref() == Some(name) && e.from != name
        })
        .map(|e| DanglingRef {
            from: e.from.clone(),
            target: name.to_owned(),
            line: e.line,
        })
        .collect();
    if !is_within(&path, &ctx.allowed_roots) {
        return Err(invalid("refusing to delete outside the agent roots", name));
    }
    std::fs::remove_file(&path).map_err(WriteError::from)?;
    write_index(
        ctx,
        &join(&[&dir, INDEX_FILE]),
        |raw| remove_index_line(raw, name),
        "",
    )?;
    Ok(DeleteResult {
        slug: name.to_owned(),
        dangling,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_slugs() {
        assert!(is_kebab("a-1-b"));
        assert!(!is_kebab("Not Kebab"));
        assert!(!is_kebab("a--b"));
        assert!(!is_kebab("-a"));
        assert!(is_plain_slug("-tmp-x"));
        assert!(!is_plain_slug(".."));
        assert!(!is_plain_slug("a/b"));
        assert!(!is_plain_slug(""));
    }
}
