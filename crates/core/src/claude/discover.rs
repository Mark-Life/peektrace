//! Find Claude transcripts: `<root>/<slug>/<id>.jsonl`, one level deep.
//! Subagent files under `<slug>/<id>/subagents/` are never listed.

use std::collections::HashSet;
use std::fs::{self, DirEntry};

use crate::model::{ProjectSlug, SessionFileRef, SessionId, SourceTag};
use crate::paths::PathStyle;

/// Paths found on disk use the platform's separator.
const STYLE: PathStyle = PathStyle::NATIVE;
use crate::roots::Source;

const JSONL: &str = ".jsonl";

/// True when the entry is `want` after following symlinks.
fn entry_is(entry: &DirEntry, path: &str, want: fn(&fs::FileType) -> bool) -> bool {
    match entry.file_type() {
        Ok(t) if t.is_symlink() => fs::metadata(path).is_ok_and(|m| want(&m.file_type())),
        Ok(t) => want(&t),
        Err(_) => false,
    }
}

/// Names in `dir`, in directory order, with UTF-8 names only. A read error
/// yields nothing.
fn entries(dir: &str) -> Vec<(String, DirEntry)> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok().map(|name| (name, e)))
        .collect()
}

/// Transcripts of one root, in directory order.
fn list_root(root: &str) -> Vec<(ProjectSlug, SessionId, String)> {
    entries(root)
        .into_iter()
        .filter(|(name, e)| entry_is(e, &STYLE.join(&[root, name]), fs::FileType::is_dir))
        .flat_map(|(slug, _)| {
            let dir = STYLE.join(&[root, &slug]);
            entries(&dir)
                .into_iter()
                .filter_map(|(name, e)| {
                    let id = name.strip_suffix(JSONL)?.to_owned();
                    let path = STYLE.join(&[&dir, &name]);
                    entry_is(&e, &path, fs::FileType::is_file).then_some((id, path))
                })
                .map(|(id, path)| (ProjectSlug::new(slug.clone()), SessionId::new(id), path))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every transcript across `sources`, deduped by path (first source wins).
/// Rows carry their source only when more than one source is configured.
pub fn list_session_files(sources: &[Source]) -> Vec<SessionFileRef> {
    let stamp = sources.len() > 1;
    let mut seen = HashSet::new();
    sources
        .iter()
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
