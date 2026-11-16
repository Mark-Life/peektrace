//! The memory link graph: body links and index lines resolved against the
//! vault's files, with broken and ambiguous links flagged and near-match
//! repair candidates.

use std::collections::{HashMap, HashSet};

use super::types::{EdgeKind, GraphData, GraphNode, IndexEntry, LinkEdge, MemoryEntry};

/// Source id of edges that come from MEMORY.md.
pub const INDEX_NODE: &str = "__index__";

const MAX_LEN_DELTA: usize = 3;
const NEAR_MATCH_DISTANCE: usize = 2;
const UNREACHABLE_DISTANCE: usize = 99;

/// Levenshtein distance over UTF-16 units, capped when lengths differ a lot.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    if a.len().abs_diff(b.len()) > MAX_LEN_DELTA {
        return UNREACHABLE_DISTANCE;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(sub);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// File slugs close to an unresolved `target`, nearest first.
fn near_matches(target: &str, slugs: &[&str]) -> Vec<String> {
    let mut hits: Vec<(usize, &str)> = slugs
        .iter()
        .filter(|s| **s != target)
        .map(|s| (edit_distance(target, s), *s))
        .filter(|(d, s)| s.contains(target) || target.contains(s) || *d <= NEAR_MATCH_DISTANCE)
        .collect();
    hits.sort_by_key(|(d, _)| *d);
    hits.into_iter().map(|(_, s)| s.to_owned()).collect()
}

/// Resolves a lowercased target against the file slugs, case-folded.
struct Resolver<'a> {
    by_norm: HashMap<String, Vec<&'a str>>,
    slugs: Vec<&'a str>,
}

impl<'a> Resolver<'a> {
    fn new(entries: &'a [MemoryEntry]) -> Self {
        let mut by_norm: HashMap<String, Vec<&str>> = HashMap::new();
        for e in entries {
            by_norm
                .entry(e.slug.to_lowercase())
                .or_default()
                .push(&e.slug);
        }
        Self {
            by_norm,
            slugs: entries.iter().map(|e| e.slug.as_str()).collect(),
        }
    }

    /// The file a target names and whether several files share that name.
    fn resolve(&self, target: &str) -> (Option<&'a str>, bool) {
        match self.by_norm.get(target).map(Vec::as_slice) {
            Some([first, rest @ ..]) => (Some(first), !rest.is_empty()),
            _ => (None, false),
        }
    }

    fn edge(&self, from: &str, to: &str, kind: EdgeKind, line: usize) -> LinkEdge {
        let (resolved, ambiguous) = self.resolve(to);
        LinkEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
            resolved_to: resolved.map(str::to_owned),
            ambiguous: ambiguous && kind != EdgeKind::Index,
            candidates: resolved.is_none().then(|| near_matches(to, &self.slugs)),
            line,
        }
    }
}

/// Every edge of the vault. Body links count toward node degrees and
/// orphans; index edges only mark which files the index reaches.
pub fn build_graph(entries: &[MemoryEntry], index_entries: &[IndexEntry]) -> GraphData {
    let resolver = Resolver::new(entries);
    let mut in_deg: HashMap<&str, usize> = HashMap::new();
    let mut out_deg: HashMap<&str, usize> = HashMap::new();
    let body: Vec<LinkEdge> = entries
        .iter()
        .flat_map(|f| f.links.iter().map(move |l| (f, l)))
        .map(|(f, l)| {
            let edge = resolver.edge(&f.slug, &l.target_slug, l.kind.into(), l.line);
            if let (Some(to), _) = resolver.resolve(&l.target_slug) {
                *out_deg.entry(&f.slug).or_default() += 1;
                if to != f.slug {
                    *in_deg.entry(to).or_default() += 1;
                }
            }
            edge
        })
        .collect();
    let index: Vec<LinkEdge> = index_entries
        .iter()
        .filter_map(|e| {
            let target = e.target_slug()?;
            Some(resolver.edge(INDEX_NODE, target, EdgeKind::Index, e.line_number))
        })
        .collect();
    let indexed: HashSet<&str> = index
        .iter()
        .filter_map(|e| e.resolved_to.as_deref())
        .filter(|s| !s.is_empty())
        .collect();
    let nodes: Vec<GraphNode> = entries
        .iter()
        .map(|f| GraphNode {
            slug: f.slug.clone(),
            kind: f.kind().unwrap_or("unknown").to_owned(),
            bytes: f.size,
            in_index: indexed.contains(f.slug.as_str()),
            in_deg: in_deg.get(f.slug.as_str()).copied().unwrap_or(0),
            out_deg: out_deg.get(f.slug.as_str()).copied().unwrap_or(0),
        })
        .collect();
    let orphans = nodes
        .iter()
        .filter(|n| n.in_deg == 0 && n.out_deg == 0)
        .map(|n| n.slug.clone())
        .collect();
    GraphData {
        nodes,
        edges: body.into_iter().chain(index).collect(),
        orphans,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(edit_distance("nestd", "nested"), 1);
        assert_eq!(edit_distance("ne", "nested"), 99);
        assert_eq!(edit_distance("", ""), 0);
    }

    #[test]
    fn candidates_sort_by_distance() {
        let slugs = ["nested", "none", "flat"];
        assert_eq!(near_matches("nestd", &slugs), ["nested"]);
        assert_eq!(near_matches("ne", &slugs), ["none", "nested"]);
        assert_eq!(near_matches("nested-ghost", &slugs), ["nested"]);
    }
}
