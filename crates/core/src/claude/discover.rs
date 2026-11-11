//! Find Claude transcripts: `<root>/<slug>/<id>.jsonl`, one level deep.
//! Subagent files under `<slug>/<id>/subagents/` are never listed.

use std::collections::HashSet;
use std::fs;

use crate::model::{ProjectSlug, SessionFileRef, SessionId, SourceTag};
use crate::paths::PathStyle;

/// Paths found on disk use the platform's separator.
const STYLE: PathStyle = PathStyle::NATIVE;
use crate::roots::Source;

const JSONL: &str = ".jsonl";

/// True when `path` is `want` after following symlinks. A failed `stat`
/// (e.g. a dir with read but no search permission) drops the entry, as in TS.
fn is(path: &str, want: fn(&fs::FileType) -> bool) -> bool {
    fs::metadata(path).is_ok_and(|m| want(&m.file_type()))
}

/// Names in `dir`, in directory order, with UTF-8 names only. A read error
/// yields nothing.
fn entries(dir: &str) -> Vec<String> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Transcripts of one root, in directory order.
fn list_root(root: &str) -> Vec<(ProjectSlug, SessionId, String)> {
    entries(root)
        .into_iter()
        .filter(|name| is(&STYLE.join(&[root, name]), fs::FileType::is_dir))
        .flat_map(|slug| {
            let dir = STYLE.join(&[root, &slug]);
            entries(&dir)
                .into_iter()
                .filter_map(|name| {
                    let id = name.strip_suffix(JSONL)?.to_owned();
                    let path = STYLE.join(&[&dir, &name]);
                    is(&path, fs::FileType::is_file).then_some((id, path))
                })
                .map(|(id, path)| (ProjectSlug::new(slug.clone()), SessionId::new(id), path))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// What makes two roots the same folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RootKey {
    #[cfg(unix)]
    Inode(u64, u64),
    Path(String),
}

/// The folder `root` names, so a case variant or symlink of a root already
/// scanned is skipped. A root that cannot be stat'ed keys on its text.
#[cfg(unix)]
fn root_key(root: &str) -> RootKey {
    use std::os::unix::fs::MetadataExt as _;
    fs::metadata(root).map_or_else(
        |_| RootKey::Path(root.to_owned()),
        |m| RootKey::Inode(m.dev(), m.ino()),
    )
}

#[cfg(not(unix))]
fn root_key(root: &str) -> RootKey {
    RootKey::Path(
        fs::canonicalize(root)
            .map_or_else(|_| root.to_owned(), |p| p.to_string_lossy().into_owned()),
    )
}

/// Every transcript across `sources`, deduped by root folder and by path
/// (first source wins). Rows carry their source only when more than one
/// source is configured.
pub fn list_session_files(sources: &[Source]) -> Vec<SessionFileRef> {
    let stamp = sources.len() > 1;
    let mut roots = HashSet::new();
    let mut seen = HashSet::new();
    sources
        .iter()
        .filter(|source| roots.insert(root_key(&source.projects_root)))
        .flat_map(|source| {
            let tag = stamp.then(|| SourceTag {
                id: source.id.clone(),
                label: source.label.clone(),
            });
            list_root(&source.projects_root)
                .into_iter()
                .map(move |(slug, id, path)| SessionFileRef {
                    id,
                    slug,
                    path,
                    source: tag.clone(),
                })
        })
        .filter(|r| seen.insert(r.path.clone()))
        .collect()
}
