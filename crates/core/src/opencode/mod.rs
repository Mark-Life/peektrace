//! `OpenCode` sessions: SQLite stores (`opencode*.db`) plus the legacy JSON
//! tree under `storage/`, deduped by session id (a DB wins). A session's
//! path is `<data dir>#<id>`.

mod db;
mod header;
mod parse;
mod session;
mod tree;

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;
use tracing::{Span, info_span};

pub use parse::parse_opencode_session;
pub use session::Session;

use crate::headers::Outcome;
use crate::model::{Agent, ProjectSlug, SessionFileRef, SessionId, SourceTag};
use crate::roots::Source;

/// Session refs of one data dir: every DB row, then tree sessions no DB has.
fn list_root(data_dir: &str) -> Vec<(String, Option<String>)> {
    let dbs = db::open_dbs(data_dir);
    let from_db = db::list_db_refs(&dbs);
    drop(dbs);
    let mut seen: HashSet<String> = from_db.iter().map(|r| r.id.clone()).collect();
    let from_tree = tree::list_tree_refs(data_dir)
        .into_iter()
        .filter(|r| seen.insert(r.id.clone()))
        .map(|r| (r.id, r.directory));
    from_db
        .into_iter()
        .map(|r| (r.id, r.directory))
        .chain(from_tree)
        .collect()
}

/// Every session across `sources`, deduped by path (first source wins).
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
            let root = source.projects_root.clone();
            list_root(&source.projects_root)
                .into_iter()
                .map(move |(id, directory)| SessionFileRef {
                    path: format!("{root}#{id}"),
                    id: SessionId::new(id),
                    slug: ProjectSlug::new(directory.unwrap_or_default()),
                    stamp: crate::model::FileStamp::default(),
                    source: tag.clone(),
                })
        })
        .filter(|r| seen.insert(r.path.clone()))
        .collect()
}

/// Data dir and session id a path names: split on the last `#`; with no
/// `#`, the default data dir and the ref's id.
fn locate<'a>(path: &'a str, id: &'a str, default_root: &'a str) -> (&'a str, &'a str) {
    path.rfind('#')
        .map_or((default_root, id), |at| (&path[..at], &path[at + 1..]))
}

/// The store files one session reads from: its data dir, each
/// `opencode*.db` there with its `-wal`, and its tree message dir. A write
/// to the session changes the size or modified time of one of them.
pub fn store_paths(path: &str, id: &str, default_root: &str) -> Vec<String> {
    let (data_dir, id) = locate(path, id, default_root);
    let style = crate::paths::PathStyle::NATIVE;
    let dbs = db::db_names(data_dir).into_iter().flat_map(|name| {
        let db = style.join(&[data_dir, &name]);
        let wal = format!("{db}-wal");
        [db, wal]
    });
    std::iter::once(data_dir.to_owned())
        .chain(dbs)
        .chain(std::iter::once(
            style.join(&[data_dir, "storage", "message", id]),
        ))
        .collect()
}

/// Load one session from DBs open on its data dir, else from the tree.
fn load_with(
    dbs: &[db::Db],
    data_dir: &str,
    id: &str,
    index: Option<&tree::TreeIndex>,
) -> Option<Session> {
    db::load_from_dbs(dbs, id).or_else(|| tree::load_from_tree(data_dir, id, index))
}

/// Load the session `path` names (see [`locate`]); `None` when no store
/// holds it.
pub fn load_session(path: &str, id: &str, default_root: &str) -> Option<Session> {
    let _span = info_span!(
        "AgentRegistry.loadTranscript",
        agent = Agent::Opencode.as_str()
    )
    .entered();
    let (data_dir, id) = locate(path, id, default_root);
    load_with(&db::open_dbs(data_dir), data_dir, id, None)
}

/// Tree indexes of the data dirs some session paths name, built once so
/// loading many sessions does not rescan every project dir per session.
pub struct Store<'a> {
    default_root: &'a str,
    indexes: HashMap<&'a str, tree::TreeIndex>,
}

impl<'a> Store<'a> {
    pub fn new(paths: impl IntoIterator<Item = &'a str>, default_root: &'a str) -> Self {
        let data_dirs: HashSet<&str> = paths
            .into_iter()
            .map(|p| locate(p, "", default_root).0)
            .collect();
        let indexes = data_dirs
            .into_par_iter()
            .map(|dir| (dir, tree::TreeIndex::build(dir)))
            .collect();
        Self {
            default_root,
            indexes,
        }
    }

    /// A loader for one worker.
    pub fn reader(&self) -> Reader<'_> {
        Reader {
            store: self,
            open: None,
        }
    }
}

/// Loads sessions of a [`Store`], keeping the DBs of the last data dir open.
pub struct Reader<'s> {
    store: &'s Store<'s>,
    open: Option<(String, Vec<db::Db>)>,
}

impl Reader<'_> {
    /// Same as [`load_session`], without reopening the DBs per session.
    pub fn load(&mut self, path: &str, id: &str) -> Option<Session> {
        let (data_dir, id) = locate(path, id, self.store.default_root);
        if self.open.as_ref().is_none_or(|(dir, _)| dir != data_dir) {
            self.open = Some((data_dir.to_owned(), db::open_dbs(data_dir)));
        }
        let dbs = self
            .open
            .as_ref()
            .map_or(&[][..], |(_, dbs)| dbs.as_slice());
        load_with(dbs, data_dir, id, self.store.indexes.get(data_dir))
    }
}

/// One outcome per ref, in `refs` order. A ref that no store can load (bad
/// message JSON, a tree file named apart from its id) is skipped.
pub fn load_headers(refs: &[SessionFileRef], default_root: &str, parent: &Span) -> Vec<Outcome> {
    let store = Store::new(refs.iter().map(|r| r.path.as_str()), default_root);
    refs.par_iter()
        .map_init(
            || store.reader(),
            |reader, r| {
                let _span = info_span!(
                    parent: parent,
                    "AgentRegistry.loadTranscript",
                    agent = Agent::Opencode.as_str()
                )
                .entered();
                reader
                    .load(&r.path, r.id.as_str())
                    .map_or(Outcome::Skip, |s| {
                        Outcome::Header(Box::new(header::build_header(&s, r)))
                    })
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_by_last_hash() {
        assert_eq!(locate("/d#ses_1", "x", "/def"), ("/d", "ses_1"));
        assert_eq!(locate("/a#b/c#ses", "x", "/def"), ("/a#b/c", "ses"));
        assert_eq!(
            locate("/oc/ses_1.jsonl", "ses_1", "/def"),
            ("/def", "ses_1")
        );
    }
}
