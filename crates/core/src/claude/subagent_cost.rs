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
use crate::model::{FileStamp, SessionHeader};
use crate::sessions::cost::header_cost;
use crate::sessions::model::TurnsCost;

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

/// Every `agent-*.jsonl` under `dir` and its subdirs, as `find_subagents`
/// walks them, with its stamp.
fn subagent_files(dir: &Path, out: &mut Vec<(PathBuf, FileStamp)>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            subagent_files(&path, out);
        } else if meta.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("agent-") && n.ends_with(".jsonl"))
        {
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
    if let Some(c) = cache.and_then(|c| c.get(path, stamp)) {
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

/// Add the session's subagent files to its cost. Returns the files priced,
/// for the cache.
fn add_to(h: &mut SessionHeader, cache: Option<&SubagentCosts>) -> Vec<(PathBuf, Priced)> {
    let Some(base) = h.cost_parts.as_deref() else {
        return Vec::new();
    };
    let Some(dir) = subagent_dir(&h.path) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    subagent_files(&dir, &mut files);
    if files.is_empty() {
        return Vec::new();
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
