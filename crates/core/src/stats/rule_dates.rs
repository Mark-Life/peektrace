//! When each memory line appeared, so a rule is dated by its own line and not
//! by its file's mtime. The ledger stores line hashes, never text.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::cache::sha1_hex;
use super::detectors::rules::MemoryDoc;
use super::rank::collapse_ws;

/// Line hash -> first-seen epoch ms (`None`: undated), per real path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineLedger {
    pub files: IndexMap<String, IndexMap<String, Option<f64>>>,
}

const LINE_HASH_LEN: usize = 16;

fn line_key(line: &str) -> Option<String> {
    let text = collapse_ws(line);
    (!text.is_empty()).then(|| sha1_hex(&text)[..LINE_HASH_LEN].to_owned())
}

/// Per real path, each line's date.
#[derive(Debug, Default)]
pub struct LineDates(HashMap<String, Vec<Option<f64>>>);

impl LineDates {
    /// Every line dated by its file's mtime.
    pub fn by_mtime() -> Self {
        Self::default()
    }

    pub fn date_of(&self, doc: &MemoryDoc, line: usize) -> Option<f64> {
        match self.0.get(&doc.real_path) {
            None => doc.mtime_ms,
            Some(days) => days.get(line).copied().flatten(),
        }
    }
}

/// Date every line of `docs` against `ledger`; returns the dates and the
/// ledger to write back.
pub fn date_lines(docs: &[MemoryDoc], ledger: &LineLedger, now: f64) -> (LineDates, LineLedger) {
    let mut files = ledger.files.clone();
    let mut dated: HashMap<String, Vec<Option<f64>>> = HashMap::new();
    for doc in docs {
        if dated.contains_key(&doc.real_path) {
            continue;
        }
        let known = ledger.files.get(&doc.real_path);
        let fresh = if known.is_none() {
            doc.mtime_ms
        } else {
            Some(doc.mtime_ms.unwrap_or(now))
        };
        let mut kept: IndexMap<String, Option<f64>> = IndexMap::new();
        let days = doc
            .lines
            .iter()
            .map(|line| {
                let key = line_key(line)?;
                *kept.entry(key.clone()).or_insert_with(|| match known {
                    Some(k) if k.contains_key(&key) => k.get(&key).copied().flatten(),
                    _ => fresh,
                })
            })
            .collect();
        dated.insert(doc.real_path.clone(), days);
        files.insert(doc.real_path.clone(), kept);
    }
    (LineDates(dated), LineLedger { files })
}
