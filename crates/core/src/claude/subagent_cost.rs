//! Subagent cost for list headers. A Claude session without a full cost
//! record adds the cost of each `agent-*.jsonl` under its subagents dir,
//! workflow agents included, as the session detail does. Each file is
//! priced once per size and mtime when a cache is given.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rayon::prelude::*;
use tracing::debug;

use super::read::subagent_cost;
use crate::discover::stamp_of;
use crate::headers::Outcome;
use crate::model::{FileStamp, HeaderCost, SessionHeader};
use crate::sessions::cost::header_cost;
use crate::sessions::model::TurnsCost;
use crate::sessions::resolve::is_subagent_file;

/// A subagent file's cost and the stamp it was priced at.
type Priced = (FileStamp, TurnsCost);

/// Subagent costs by file path. Like the header cache, a race between two
/// lists costs an extra read, never a wrong cost: every hit is checked by
/// its stamp.
#[derive(Debug, Default)]
pub struct SubagentCosts(Mutex<HashMap<PathBuf, Priced>>);

impl SubagentCosts {
    fn get(&self, path: &Path, stamp: FileStamp) -> Option<TurnsCost> {
        let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.get(path)
            .filter(|(s, _)| *s == stamp)
            .map(|(_, c)| c.clone())
    }

    /// Keep `fresh`; with `complete`, drop every file not in it.
    fn keep(&self, fresh: HashMap<PathBuf, Priced>, complete: bool) {
        let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if complete {
            *all = fresh;
        } else {
            all.extend(fresh);
        }
    }
}

/// Subdirs deeper than this are not walked, so a symlink loop ends.
const MAX_DEPTH: usize = 8;

/// Every `agent-*.jsonl` under `dir` and its subdirs, as `find_subagents`
/// walks them, with its stamp.
fn subagent_files(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, FileStamp)>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                subagent_files(&path, depth + 1, out);
            }
        } else if meta.is_file() && entry.file_name().to_str().is_some_and(is_subagent_file) {
            out.push((path, stamp_of(&meta)));
        }
    }
}

/// `<project>/<id>/subagents` for the transcript `<project>/<id>.jsonl`.
fn subagent_dir(transcript: &str) -> Option<PathBuf> {
    let stem = transcript.strip_suffix(".jsonl")?;
    Some(Path::new(stem).join("subagents"))
}

/// One subagent file's cost: cached at this stamp, else read. `None` when
/// the file cannot be read.
fn file_cost(path: &Path, stamp: FileStamp, cache: Option<&SubagentCosts>) -> Option<TurnsCost> {
    // A chmod keeps size and mtime, so a hit counts only while the file
    // still opens.
    if let Some(c) = cache.and_then(|c| c.get(path, stamp))
        && fs::File::open(path).is_ok()
    {
        return Some(c);
    }
    match subagent_cost(path) {
        Ok(c) => Some(c),
        Err(e) => {
            debug!(path = %path.display(), reason = %e, "subagent unreadable");
            None
        }
    }
}

/// Set the session's cost from its parts plus its subagent files. Returns
/// the files priced, for the cache.
fn add_to(h: &mut SessionHeader, cache: Option<&SubagentCosts>) -> Vec<(PathBuf, Priced)> {
    let Some(base) = h.cost_parts.as_deref() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    if let Some(dir) = subagent_dir(&h.path) {
        subagent_files(&dir, 0, &mut files);
    }
    let priced: Vec<_> = files
        .par_iter()
        .map(|(path, stamp)| (path, *stamp, file_cost(path, *stamp, cache)))
        .collect();
    let mut parts = base.clone();
    let mut seen = Vec::with_capacity(priced.len());
    for (path, stamp, cost) in priced {
        match cost {
            Some(c) => {
                seen.push((path.clone(), (stamp, c.clone())));
                parts.parts.push(c);
            }
            None => parts.unreadable += 1,
        }
    }
    h.cost = header_cost(&parts);
    seen
}

/// Add subagent costs to headers listed without them, reading every file:
/// for a one-shot list that prices only the rows it shows.
pub fn add_subagent_costs_to(headers: &mut [SessionHeader]) {
    headers.par_iter_mut().for_each(|h| {
        add_to(h, None);
    });
}

/// Leave unknown every Claude cost that subagent files could still add to.
pub fn drop_pending_costs(outcomes: &mut [Outcome]) {
    for o in outcomes {
        if let Outcome::Header(h) = o
            && h.cost_parts.is_some()
        {
            h.cost = HeaderCost::default();
        }
    }
}

/// Add subagent costs to every Claude header that still needs them.
/// `complete` says the outcomes cover every Claude session, so cached files
/// not seen drop out.
pub fn add_subagent_costs(outcomes: &mut [Outcome], cache: Option<&SubagentCosts>, complete: bool) {
    let seen: Vec<_> = outcomes
        .par_iter_mut()
        .flat_map_iter(|o| match o {
            Outcome::Header(h) => add_to(h, cache),
            Outcome::Skip | Outcome::Unreadable(_) => Vec::new(),
        })
        .collect();
    if let Some(cache) = cache {
        cache.keep(seen.into_iter().collect(), complete);
    }
}
