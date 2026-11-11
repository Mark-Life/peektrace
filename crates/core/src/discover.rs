//! Find transcripts on disk for every file-backed agent: the slug layout
//! (`<root>/<slug>/<file>.jsonl`, Claude and Pi) and the Codex date tree.

use std::collections::HashSet;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{FileStamp, ProjectSlug, SessionFileRef, SessionId, SourceTag};
use crate::paths::PathStyle;
use crate::roots::Source;

/// Paths found on disk use the platform's separator.
const STYLE: PathStyle = PathStyle::NATIVE;

pub(crate) const JSONL: &str = ".jsonl";

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
pub(crate) struct Found {
    slug: ProjectSlug,
    id: SessionId,
    path: String,
    stamp: FileStamp,
}

/// Turns a transcript file name into its session id.
pub(crate) type IdOf = fn(&str) -> String;

/// The file name without `.jsonl`.
pub(crate) fn stem(name: &str) -> String {
    name.strip_suffix(JSONL).unwrap_or(name).to_owned()
}

/// Transcripts in one slug dir, in directory order.
fn list_slug(root: &str, slug: &str, id_of: IdOf) -> Vec<Found> {
    let dir = STYLE.join(&[root, slug]);
    entries(&dir)
        .into_iter()
        .filter_map(|name| {
            name.strip_suffix(JSONL)?;
            let path = STYLE.join(&[&dir, &name]);
            let meta = stat_as(&path, fs::FileType::is_file)?;
            Some(Found {
                slug: ProjectSlug::new(slug),
                id: SessionId::new(id_of(&name)),
                stamp: stamp_of(&meta),
                path,
            })
        })
        .collect()
}

/// Transcripts of one slug-layout root, in directory order. A non-empty
/// `project` skips every other slug dir before it is stat'ed or read.
pub(crate) fn slug_layout(root: &str, project: Option<&str>, id_of: IdOf) -> Vec<Found> {
    let project = project.filter(|p| !p.is_empty());
    entries(root)
        .into_iter()
        .filter(|name| project.is_none_or(|p| p == name))
        .filter(|name| stat_as(&STYLE.join(&[root, name]), fs::FileType::is_dir).is_some())
        .flat_map(|slug| list_slug(root, &slug, id_of))
        .collect()
}

/// Every file under `dir` whose name passes `keep`, depth first in
/// directory order, following symlinks. A dir already on the walk's path (a
/// symlink loop) is not entered again.
pub(crate) fn walk_tree(dir: &str, keep: fn(&str) -> bool, id_of: IdOf) -> Vec<Found> {
    fn walk(
        dir: &str,
        keep: fn(&str) -> bool,
        id_of: IdOf,
        up: &mut Vec<RootKey>,
        out: &mut Vec<Found>,
    ) {
        let key = root_key(dir);
        if up.contains(&key) {
            return;
        }
        up.push(key);
        for name in entries(dir) {
            let path = STYLE.join(&[dir, &name]);
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                walk(&path, keep, id_of, up, out);
            } else if meta.is_file() && keep(&name) {
                out.push(Found {
                    slug: ProjectSlug::new(""),
                    id: SessionId::new(id_of(&name)),
                    stamp: stamp_of(&meta),
                    path,
                });
            }
        }
        up.pop();
    }
    let mut out = Vec::new();
    walk(dir, keep, id_of, &mut Vec::new(), &mut out);
    out
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

/// Every transcript across `sources` as `list_root` finds them, deduped by
/// root folder and by path (first source wins). Rows carry their source
/// only when more than one source is configured.
pub(crate) fn list_sources(
    sources: &[Source],
    list_root: impl Fn(&str) -> Vec<Found>,
) -> Vec<SessionFileRef> {
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
            home: String::new(),
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
        let list = |project| list_sources(&sources, |r| slug_layout(r, project, stem));
        let slugs = |project| {
            let mut slugs = list(project)
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
        let [found] = list(Some("-a"))
            .try_into()
            .map_err(|_| std::io::Error::other("expected one ref"))?;
        let meta = fs::metadata(root.join("-a/s1.jsonl"))?;
        assert_eq!(found.id.as_str(), "s1");
        assert_eq!(found.stamp, stamp_of(&meta));
        assert_eq!(found.stamp.size_bytes, 3);
        fs::remove_dir_all(&root)
    }

    #[cfg(unix)]
    #[test]
    fn tree_walk_lists_every_depth_and_stops_at_loops() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("peektrace-walk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("2026/01/x.jsonl"))?;
        fs::write(root.join("top.jsonl"), "")?;
        fs::write(root.join("2026/01/deep.jsonl"), "")?;
        fs::write(root.join("2026/01/x.jsonl/in.jsonl"), "")?;
        fs::write(root.join("2026/01/skip.txt"), "")?;
        std::os::unix::fs::symlink(root.join("2026"), root.join("2026/01/loop"))?;
        std::os::unix::fs::symlink(root.join("2026/01"), root.join("alias"))?;
        let found = walk_tree(&root.to_string_lossy(), |n| n.ends_with(JSONL), stem);
        let mut names: Vec<_> = found
            .iter()
            .map(|f| f.path[root.to_string_lossy().len()..].to_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "/2026/01/deep.jsonl",
                "/2026/01/x.jsonl/in.jsonl",
                "/alias/deep.jsonl",
                "/alias/x.jsonl/in.jsonl",
                "/top.jsonl",
            ]
        );
        fs::remove_dir_all(&root)
    }
}
