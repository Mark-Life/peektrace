//! Find transcripts on disk for every file-backed agent: the slug layout
//! (`<root>/<slug>/<file>.jsonl`, Claude and Pi) and the Codex date tree.

use std::collections::HashSet;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use rayon::prelude::*;

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

/// One directory entry with the type `read_dir` reported (not followed).
struct Entry {
    name: String,
    kind: fs::FileType,
}

impl Entry {
    /// A dir, following a symlink with one `stat`; any other entry costs
    /// no syscall.
    fn is_dir(&self, path: &str) -> bool {
        if self.kind.is_symlink() {
            stat_as(path, fs::FileType::is_dir).is_some()
        } else {
            self.kind.is_dir()
        }
    }
}

/// Entries in `dir`, in directory order, with UTF-8 names only. A read
/// error yields nothing.
fn entries(dir: &str) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|e| {
            let kind = e.file_type().ok()?;
            let name = e.file_name().into_string().ok()?;
            Some(Entry { name, kind })
        })
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
        .filter_map(|Entry { name, kind }| {
            name.strip_suffix(JSONL)?;
            if kind.is_dir() {
                return None;
            }
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
/// `project` skips every other slug dir before it is stat'ed or read. Slug
/// dirs are read in parallel: a Pi root holds thousands, mostly empty.
pub(crate) fn slug_layout(root: &str, project: Option<&str>, id_of: IdOf) -> Vec<Found> {
    let project = project.filter(|p| !p.is_empty());
    entries(root)
        .into_par_iter()
        .filter(|e| project.is_none_or(|p| p == e.name))
        .filter(|e| e.is_dir(&STYLE.join(&[root, &e.name])))
        .flat_map_iter(|e| list_slug(root, &e.name, id_of))
        .collect()
}

/// Every file under `dir` whose name passes `keep`, depth first in
/// directory order, following symlinks. A dir already on the walk's path (a
/// symlink loop) is not entered again. Sibling entries are stat'ed and
/// walked in parallel: a Codex tree holds one dir per day.
pub(crate) fn walk_tree(dir: &str, keep: fn(&str) -> bool, id_of: IdOf) -> Vec<Found> {
    walk(dir, keep, id_of, &[])
}

/// `walk_tree` below `dir`, where `up` holds the dirs on the path to it.
fn walk(dir: &str, keep: fn(&str) -> bool, id_of: IdOf, up: &[RootKey]) -> Vec<Found> {
    let key = root_key(dir);
    if up.contains(&key) {
        return Vec::new();
    }
    let up = [up, &[key]].concat();
    entries(dir)
        .into_par_iter()
        .flat_map_iter(|entry| {
            let path = STYLE.join(&[dir, &entry.name]);
            if entry.is_dir(&path) {
                return walk(&path, keep, id_of, &up);
            }
            if !keep(&entry.name) {
                return Vec::new();
            }
            stat_as(&path, fs::FileType::is_file)
                .map(|meta| Found {
                    slug: ProjectSlug::new(""),
                    id: SessionId::new(id_of(&entry.name)),
                    stamp: stamp_of(&meta),
                    path,
                })
                .into_iter()
                .collect()
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

    fn temp(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("peektrace-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    /// Paths under `root`, with `/` separators, sorted.
    fn rel_paths(root: &std::path::Path, found: &[Found]) -> Vec<String> {
        let prefix = root.to_string_lossy().len();
        let mut names: Vec<_> = found
            .iter()
            .map(|f| f.path[prefix..].replace('\\', "/"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn slug_layout_keeps_directory_order() -> std::io::Result<()> {
        let root = temp("slug-order");
        for n in 0..64 {
            let slug = root.join(format!("-p{n}"));
            fs::create_dir_all(&slug)?;
            if n % 3 == 0 {
                fs::write(slug.join("a.jsonl"), "")?;
                fs::write(slug.join("b.jsonl"), "")?;
            }
        }
        fs::create_dir_all(root.join("-p1/dir.jsonl"))?;
        fs::write(root.join("loose.jsonl"), "")?;
        let root_str = root.to_string_lossy().into_owned();
        let want: Vec<_> = entries(&root_str)
            .into_iter()
            .filter(|e| e.kind.is_dir())
            .flat_map(|e| list_slug(&root_str, &e.name, stem))
            .map(|f| f.path)
            .collect();
        let got: Vec<_> = slug_layout(&root_str, None, stem)
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(got.len(), 44);
        assert_eq!(got, want);
        fs::remove_dir_all(&root)
    }

    #[test]
    fn tree_walk_lists_every_depth() -> std::io::Result<()> {
        let root = temp("walk");
        fs::create_dir_all(root.join("2026/01/x.jsonl"))?;
        fs::write(root.join("top.jsonl"), "")?;
        fs::write(root.join("2026/01/deep.jsonl"), "")?;
        fs::write(root.join("2026/01/x.jsonl/in.jsonl"), "")?;
        fs::write(root.join("2026/01/skip.txt"), "")?;
        let found = walk_tree(&root.to_string_lossy(), |n| n.ends_with(JSONL), stem);
        assert_eq!(
            rel_paths(&root, &found),
            [
                "/2026/01/deep.jsonl",
                "/2026/01/x.jsonl/in.jsonl",
                "/top.jsonl",
            ]
        );
        fs::remove_dir_all(&root)
    }

    /// Kept paths under `dir`, depth first in directory order, one at a time.
    fn walk_in_order(dir: &str) -> Vec<String> {
        entries(dir)
            .into_iter()
            .flat_map(|e| {
                let path = STYLE.join(&[dir, &e.name]);
                if e.kind.is_dir() {
                    walk_in_order(&path)
                } else if e.name.ends_with(JSONL) {
                    vec![path]
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    #[test]
    fn tree_walk_keeps_directory_order() -> std::io::Result<()> {
        let root = temp("walk-order");
        for (month, day) in (1..=6).flat_map(|m| (1..=12).map(move |d| (m, d))) {
            let dir = root.join(format!("2026/{month:02}/{day:02}"));
            fs::create_dir_all(&dir)?;
            for n in 0..(day % 4) {
                fs::write(dir.join(format!("rollout-{n}.jsonl")), "")?;
            }
            fs::write(dir.join("note.txt"), "")?;
        }
        fs::write(root.join("2026/top.jsonl"), "")?;
        let root_str = root.to_string_lossy().into_owned();
        let got: Vec<_> = walk_tree(&root_str, |n| n.ends_with(JSONL), stem)
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(got.len(), 6 * 18 + 1);
        assert_eq!(got, walk_in_order(&root_str));
        fs::remove_dir_all(&root)
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed_and_loops_stop() -> std::io::Result<()> {
        use std::os::unix::fs::symlink;
        let root = temp("walk-links");
        fs::create_dir_all(root.join("2026/01/x.jsonl"))?;
        fs::write(root.join("2026/01/deep.jsonl"), "")?;
        fs::write(root.join("2026/01/x.jsonl/in.jsonl"), "")?;
        symlink(root.join("2026"), root.join("2026/01/loop"))?;
        symlink(root.join("2026/01"), root.join("alias"))?;
        symlink(root.join("2026/01/deep.jsonl"), root.join("linked.jsonl"))?;
        let found = walk_tree(&root.to_string_lossy(), |n| n.ends_with(JSONL), stem);
        assert_eq!(
            rel_paths(&root, &found),
            [
                "/2026/01/deep.jsonl",
                "/2026/01/x.jsonl/in.jsonl",
                "/alias/deep.jsonl",
                "/alias/x.jsonl/in.jsonl",
                "/linked.jsonl",
            ]
        );
        let slugs = slug_layout(&root.to_string_lossy(), None, stem);
        assert_eq!(rel_paths(&root, &slugs), ["/alias/deep.jsonl"]);
        fs::remove_dir_all(&root)
    }
}
