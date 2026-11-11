//! Find Claude transcripts: `<root>/<slug>/<id>.jsonl`, one level deep.
//! Subagent files under `<slug>/<id>/subagents/` are never listed.

use std::collections::HashSet;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{FileStamp, ProjectSlug, SessionFileRef, SessionId, SourceTag};
use crate::paths::PathStyle;
use crate::roots::Source;

/// Paths found on disk use the platform's separator.
const STYLE: PathStyle = PathStyle::NATIVE;

const JSONL: &str = ".jsonl";

/// Milliseconds since the epoch, truncated toward zero like a JS `Date`.
fn epoch_ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// Size and mtime of a stat result. A missing mtime reads as 0.
fn stamp_of(meta: &fs::Metadata) -> FileStamp {
    FileStamp {
        size_bytes: meta.len(),
        mtime_ms: meta.modified().map_or(0, epoch_ms),
    }
}

/// `path` after following symlinks, when its type passes `want`. A failed
/// `stat` (e.g. a dir with read but no search permission) drops the entry,
/// as in TS.
fn stat_as(path: &str, want: fn(&fs::FileType) -> bool) -> Option<fs::Metadata> {
    fs::metadata(path).ok().filter(|m| want(&m.file_type()))
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

/// One transcript found under a root, before its source is attached.
struct Found {
    slug: ProjectSlug,
    id: SessionId,
    path: String,
    stamp: FileStamp,
}

/// Transcripts in one slug dir, in directory order.
fn list_slug(root: &str, slug: &str) -> Vec<Found> {
    let dir = STYLE.join(&[root, slug]);
    entries(&dir)
        .into_iter()
        .filter_map(|name| {
            let id = name.strip_suffix(JSONL)?;
            let path = STYLE.join(&[&dir, &name]);
            let meta = stat_as(&path, fs::FileType::is_file)?;
            Some(Found {
                slug: ProjectSlug::new(slug),
                id: SessionId::new(id),
                stamp: stamp_of(&meta),
                path,
            })
        })
        .collect()
}

/// Transcripts of one root, in directory order. A non-empty `project` skips
/// every other slug dir before it is stat'ed or read.
fn list_root(root: &str, project: Option<&str>) -> Vec<Found> {
    let project = project.filter(|p| !p.is_empty());
    entries(root)
        .into_iter()
        .filter(|name| project.is_none_or(|p| p == name))
        .filter(|name| stat_as(&STYLE.join(&[root, name]), fs::FileType::is_dir).is_some())
        .flat_map(|slug| list_slug(root, &slug))
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
/// (first source wins), limited to the `project` slug when one is given.
/// Rows carry their source only when more than one source is configured.
pub fn list_session_files(sources: &[Source], project: Option<&str>) -> Vec<SessionFileRef> {
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
            list_root(&source.projects_root, project)
                .into_iter()
                .map(move |f| SessionFileRef {
                    id: f.id,
                    slug: f.slug,
                    path: f.path,
                    stamp: f.stamp,
                    source: tag.clone(),
                })
        })
        .filter(|r| seen.insert(r.path.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(root: &str) -> Source {
        Source {
            id: "claude".to_owned(),
            label: "Claude".to_owned(),
            projects_root: root.to_owned(),
        }
    }

    #[test]
    fn project_and_stamps_come_from_discovery() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("peektrace-discover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("-a"))?;
        fs::create_dir_all(root.join("-b"))?;
        fs::write(root.join("-a/s1.jsonl"), "abc")?;
        fs::write(root.join("-b/s2.jsonl"), "")?;
        let root_str = root.to_string_lossy().into_owned();
        let sources = [source(&root_str)];
        let slugs = |project| {
            let mut slugs = list_session_files(&sources, project)
                .into_iter()
                .map(|r| r.slug.as_str().to_owned())
                .collect::<Vec<_>>();
            slugs.sort();
            slugs
        };
        assert_eq!(slugs(None), ["-a", "-b"]);
        assert_eq!(slugs(Some("")), ["-a", "-b"]);
        assert_eq!(slugs(Some("-b")), ["-b"]);
        assert_eq!(slugs(Some("-c")), Vec::<String>::new());
        let [found] = list_session_files(&sources, Some("-a"))
            .try_into()
            .map_err(|_| std::io::Error::other("expected one ref"))?;
        let meta = fs::metadata(root.join("-a/s1.jsonl"))?;
        assert_eq!(found.id.as_str(), "s1");
        assert_eq!(found.stamp, stamp_of(&meta));
        assert_eq!(found.stamp.size_bytes, 3);
        fs::remove_dir_all(&root)
    }
}
