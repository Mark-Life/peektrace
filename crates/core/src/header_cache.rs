//! List headers kept in memory between listings, per transcript file.
//!
//! A file's header is reused while its discovery size and mtime match the
//! stamp it was parsed with, so a refresh re-reads only new or changed files.
//! A failed read is never cached, and a hit counts only while the file still
//! opens, so an unreadable file is reported on every list.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::headers::{Outcome, unreadable};
use crate::model::{Agent, FileStamp, ProjectSlug, SessionFileRef, SessionHeader, SessionId};

/// `buildHeader` reads the id and slug as well as the text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    path: String,
    id: SessionId,
    slug: ProjectSlug,
}

impl Key {
    fn of(r: &SessionFileRef) -> Self {
        Self {
            path: r.path.clone(),
            id: r.id.clone(),
            slug: r.slug.clone(),
        }
    }
}

/// A header, or `None` for an empty file, and the stamp it was built from.
#[derive(Debug, Clone)]
struct Entry {
    stamp: FileStamp,
    header: Option<Box<SessionHeader>>,
}

type AgentEntries = HashMap<Key, Entry>;

/// Cached headers per agent. Concurrent lists race last-writer-wins; every
/// entry is checked by its stamp on read, so a race costs extra parses, never
/// a wrong header.
#[derive(Debug, Default)]
pub struct HeaderCache(Mutex<HashMap<Agent, AgentEntries>>);

/// What a cached entry yields for `r` now.
fn reuse(agent: Agent, r: &SessionFileRef, entry: &Entry) -> Outcome {
    // A chmod keeps size and mtime, so check the file still opens.
    if let Err(e) = std::fs::File::open(&r.path) {
        return unreadable(agent, r, &e);
    }
    entry.header.clone().map_or(Outcome::Skip, |mut h| {
        h.source.clone_from(&r.source);
        Outcome::Header(h)
    })
}

/// The entry worth keeping for a fresh outcome; failed reads are not kept.
fn entry_of(r: &SessionFileRef, o: &Outcome) -> Option<Entry> {
    let header = match o {
        Outcome::Header(h) => Some(h.clone()),
        Outcome::Skip => None,
        Outcome::Unreadable(_) => return None,
    };
    Some(Entry {
        stamp: r.stamp,
        header,
    })
}

impl HeaderCache {
    /// One outcome per ref, in `refs` order: cached when the stamp matches,
    /// else from `load`, which gets only the refs to read. `complete` says
    /// `refs` is every file of the agent, so entries not seen drop out.
    pub(crate) fn outcomes(
        &self,
        agent: Agent,
        refs: &[SessionFileRef],
        complete: bool,
        load: impl FnOnce(&[SessionFileRef]) -> Vec<Outcome>,
    ) -> Vec<Outcome> {
        let hits: Vec<Option<Entry>> = {
            let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            let cached = all.get(&agent);
            refs.iter()
                .map(|r| {
                    cached
                        .and_then(|c| c.get(&Key::of(r)))
                        .filter(|e| e.stamp == r.stamp)
                        .cloned()
                })
                .collect()
        };
        let misses: Vec<SessionFileRef> = refs
            .iter()
            .zip(&hits)
            .filter(|(_, h)| h.is_none())
            .map(|(r, _)| r.clone())
            .collect();
        let mut loaded = load(&misses).into_iter();
        let (outcomes, fresh): (Vec<Outcome>, AgentEntries) = refs.iter().zip(hits).fold(
            (Vec::with_capacity(refs.len()), HashMap::new()),
            |(mut outcomes, mut fresh), (r, hit)| {
                let (outcome, entry) = if let Some(e) = hit {
                    let o = reuse(agent, r, &e);
                    let keep = !matches!(o, Outcome::Unreadable(_));
                    (o, keep.then_some(e))
                } else {
                    let o = loaded.next().unwrap_or(Outcome::Skip);
                    let e = entry_of(r, &o);
                    (o, e)
                };
                if let Some(e) = entry {
                    fresh.insert(Key::of(r), e);
                }
                outcomes.push(outcome);
                (outcomes, fresh)
            },
        );
        let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if complete {
            all.insert(agent, fresh);
        } else {
            all.entry(agent).or_default().extend(fresh);
        }
        outcomes
    }
}
