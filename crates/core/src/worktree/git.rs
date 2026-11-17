//! Linked git worktrees: a checkout whose `.git` is a file naming
//! `<main>/.git/worktrees/<name>`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

/// A `.git` file is one line; a bigger one is not a worktree link.
const MAX_LINK_BYTES: u64 = 4096;

/// Cached dirs before the cache starts over.
const MAX_CACHED: usize = 4096;

/// What git says about a linked worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The main checkout's root.
    pub root: String,
    /// The checked-out branch; `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// The worktree's admin dir, `<main>/.git/worktrees/<name>`.
    gitdir: PathBuf,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The branch a `HEAD` file names, `None` when detached or unreadable.
fn head_branch(gitdir: &Path) -> Option<String> {
    let head = fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?;
    (!branch.is_empty()).then(|| branch.to_owned())
}

/// The main checkout of an admin dir: `<root>/.git/worktrees/<name>` gives
/// `<root>`; a bare repo `<root>.git/worktrees/<name>` gives `<root>.git`.
fn main_root(gitdir: &Path) -> Option<String> {
    let worktrees = gitdir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    let common = worktrees.parent()?;
    let root = if common.file_name()? == ".git" {
        common.parent()?
    } else {
        common
    };
    Some(root.to_string_lossy().into_owned())
}

/// Read `<dir>/.git` as a worktree link. `None` for a main checkout, a
/// submodule, a missing dir or any read error.
pub fn read_linked(dir: &str) -> Option<Linked> {
    let dot_git = Path::new(dir).join(".git");
    let meta = fs::metadata(&dot_git).ok()?;
    if !meta.is_file() || meta.len() > MAX_LINK_BYTES {
        return None;
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text
        .lines()
        .find_map(|l| l.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|t| !t.is_empty())?;
    let gitdir = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        fs::canonicalize(Path::new(dir).join(target)).ok()?
    };
    Some(Linked {
        root: main_root(&gitdir)?,
        branch: head_branch(&gitdir),
        gitdir,
    })
}

/// A cached read and the mtimes of `.git` and `HEAD` it was made from.
type Entry = (Option<SystemTime>, Option<SystemTime>, Option<Linked>);

static CACHE: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

/// [`read_linked`], reused while `<dir>/.git` and the worktree's `HEAD`
/// keep their mtimes.
pub fn linked(dir: &str) -> Option<Linked> {
    let git_mtime = mtime(&Path::new(dir).join(".git"));
    let head_mtime = |l: &Option<Linked>| l.as_ref().and_then(|l| mtime(&l.gitdir.join("HEAD")));
    let hit = CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|c| c.get(dir).cloned());
    if let Some((g, h, found)) = hit
        && g == git_mtime
        && h == head_mtime(&found)
    {
        return found;
    }
    let found = git_mtime.and_then(|_| read_linked(dir));
    let entry = (git_mtime, head_mtime(&found), found.clone());
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    let map = cache.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_CACHED {
        map.clear();
    }
    map.insert(dir.to_owned(), entry);
    found
}
