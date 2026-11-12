//! Discovery for stats: every main transcript plus the subagent and
//! workflow-agent transcripts nested under each session dir.

use std::collections::HashSet;
use std::fs;

use rayon::prelude::*;
use tracing::info_span;

use super::rank::cmp_str;
use super::schema::Kind;
use crate::listing::{AgentRoots, session_files};
use crate::model::{Agent, SourceTag};
use crate::roots::Source;
use crate::sessions::resolve::project_slugs;

const JOURNAL: &str = "journal.jsonl";
const JSONL: &str = ".jsonl";
/// Dir levels walked under a project before giving up (symlink loops).
const MAX_DEPTH: usize = 64;

/// One transcript, classified by where it sits in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptRef {
    pub id: String,
    pub slug: String,
    pub path: String,
    pub source: Option<SourceTag>,
    pub agent: Agent,
    pub kind: Kind,
    pub parent: Option<String>,
    pub run: Option<String>,
}

/// Kind, parent and run from path parts relative to the project dir.
pub fn classify_parts(parts: &[String]) -> (Kind, Option<String>, Option<String>) {
    if parts.len() <= 1 {
        return (Kind::Main, None, None);
    }
    let parent = Some(parts[0].clone());
    let wf = parts.iter().position(|p| p.starts_with("wf_"));
    match wf {
        Some(i) if i > 0 && parts[i - 1] == "workflows" => {
            (Kind::WorkflowAgent, parent, Some(parts[i].clone()))
        }
        _ => (Kind::Subagent, parent, None),
    }
}

/// `.jsonl` files at depth >= 1 under `dir`, with their path parts. The entry
/// type comes from the dir listing; only symlinks are followed with a stat.
fn nested_in(dir: &str, parts: &mut Vec<String>, out: &mut Vec<(String, Vec<String>)>) {
    if parts.len() > MAX_DEPTH {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    let entries: Vec<(String, fs::FileType)> = read
        .filter_map(Result::ok)
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?)))
        .collect();
    for (name, kind) in entries {
        let wanted_file = !parts.is_empty() && name != JOURNAL && name.ends_with(JSONL);
        if kind.is_file() && !wanted_file {
            continue;
        }
        let path = format!("{dir}/{name}");
        let (is_dir, is_file) = if kind.is_symlink() {
            match fs::metadata(&path) {
                Ok(m) => (m.is_dir(), m.is_file()),
                Err(_) => continue,
            }
        } else {
            (kind.is_dir(), kind.is_file())
        };
        if is_dir {
            parts.push(name);
            nested_in(&path, parts, out);
            parts.pop();
        } else if is_file && wanted_file {
            let mut p = parts.clone();
            p.push(name);
            out.push((path, p));
        }
    }
}

fn nested_refs(sources: &[Source]) -> Vec<TranscriptRef> {
    let slugs = project_slugs(sources);
    sources
        .iter()
        .enumerate()
        .flat_map(|(i, source)| {
            let tag = (i > 0).then(|| SourceTag {
                id: source.id.clone(),
                label: source.label.clone(),
            });
            slugs
                .par_iter()
                .flat_map_iter(|slug| {
                    let mut found = Vec::new();
                    nested_in(
                        &format!("{}/{slug}", source.projects_root),
                        &mut Vec::new(),
                        &mut found,
                    );
                    let tag = tag.clone();
                    found.into_iter().map(move |(path, parts)| {
                        let name = parts.last().map_or("", String::as_str);
                        let id = name.strip_suffix(JSONL).unwrap_or(name).to_owned();
                        let (kind, parent, run) = classify_parts(&parts);
                        TranscriptRef {
                            id,
                            slug: slug.clone(),
                            path,
                            source: tag.clone(),
                            agent: Agent::Claude,
                            kind,
                            parent,
                            run,
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn sort_by_path(refs: &mut [TranscriptRef]) {
    refs.sort_by(|a, b| cmp_str(&a.path, &b.path));
}

/// Every transcript of one agent, path-sorted: the listing's mains plus,
/// for Claude, the nested subagent and workflow-agent transcripts.
pub fn list_transcripts(agent: Agent, roots: &AgentRoots) -> Vec<TranscriptRef> {
    let _span = info_span!("Stats.listTranscripts", agent = agent.as_str()).entered();
    let mains = session_files(agent, roots, None);
    let nested = if agent == Agent::Claude {
        nested_refs(&roots.claude)
    } else {
        Vec::new()
    };
    let mut seen = HashSet::new();
    let mut out: Vec<TranscriptRef> = mains
        .into_iter()
        .filter(|r| !r.path.ends_with(JOURNAL))
        .map(|r| TranscriptRef {
            id: r.id.as_str().to_owned(),
            slug: r.slug.as_str().to_owned(),
            path: r.path,
            source: r.source,
            agent,
            kind: Kind::Main,
            parent: None,
            run: None,
        })
        .chain(nested)
        .filter(|r| seen.insert(r.path.clone()))
        .collect();
    sort_by_path(&mut out);
    out
}

/// Every transcript across `scope`, path-sorted as one corpus.
pub fn list_corpus(scope: &[Agent], roots: &AgentRoots) -> Vec<TranscriptRef> {
    let _span = info_span!("Stats.listCorpus").entered();
    let mut out: Vec<TranscriptRef> = scope
        .iter()
        .flat_map(|a| list_transcripts(*a, roots))
        .collect();
    sort_by_path(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn classifies() {
        assert_eq!(classify_parts(&parts(&["a.jsonl"])).0, Kind::Main);
        assert_eq!(
            classify_parts(&parts(&["id", "subagents", "agent-1.jsonl"])),
            (Kind::Subagent, Some("id".into()), None)
        );
        assert_eq!(
            classify_parts(&parts(&[
                "id",
                "subagents",
                "workflows",
                "wf_x",
                "agent-1.jsonl"
            ])),
            (Kind::WorkflowAgent, Some("id".into()), Some("wf_x".into()))
        );
    }
}
