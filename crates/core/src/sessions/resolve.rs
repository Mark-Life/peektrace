//! Find a transcript by id, unique id prefix or `.jsonl` path, and the
//! subagent transcripts stored beside a Claude session.

use std::fs;

use tracing::{debug, info_span};

use super::workflows::{WorkflowRun, parse_workflow_run, to_agent_ref};
use crate::js::value::JsValue;
use crate::listing::{AgentRoots, default_root, session_files};
use crate::model::{Agent, ProjectSlug, SessionFileRef, SessionId};
use crate::roots::Source;
use crate::sessions::jsonl::decode;
use peektrace_protocol::ambiguous_message;

const JSONL: &str = ".jsonl";
const WORKFLOW_RUN: &str = "wf_";

/// A lookup or read failure, worded as the TS CLI prints it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("Session \"{id}\" not found (searched {searched}).")]
    NotFound { id: String, searched: String },
    #[error("{}", ambiguous_message(id, candidates, *count))]
    Ambiguous {
        id: String,
        /// The first [`MAX_CANDIDATES`] matching ids, sorted.
        candidates: Vec<String>,
        /// Every match, listed or not.
        count: u64,
    },
    #[error("Failed to parse transcript {path}: {reason}")]
    Read { path: String, reason: String },
}

/// Most candidate ids an ambiguity error carries.
pub const MAX_CANDIDATES: usize = 10;

impl SessionError {
    /// [`Self::Ambiguous`] for `id` from every matching id.
    pub fn ambiguous(id: &str, mut matches: Vec<String>) -> Self {
        matches.sort();
        matches.dedup();
        let count = u64::try_from(matches.len()).unwrap_or(u64::MAX);
        matches.truncate(MAX_CANDIDATES);
        Self::Ambiguous {
            id: id.to_owned(),
            candidates: matches,
            count,
        }
    }
}

/// `parent/agent-x`, as `stats drill` prints a subagent, split in two.
pub fn split_nested_id(id: &str) -> Option<(&str, &str)> {
    if id.ends_with(JSONL) {
        return None;
    }
    let (parent, agent) = id.split_once('/')?;
    let valid = !parent.is_empty() && agent.starts_with("agent-") && !agent.contains('/');
    valid.then_some((parent, agent))
}

/// `parts.join("/")` with repeated slashes collapsed.
pub fn slash_join(parts: &[&str]) -> String {
    let joined = parts.join("/");
    let mut out = String::with_capacity(joined.len());
    for c in joined.chars() {
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    out
}

/// True for a path separator of the platform (`\\` also counts on Windows).
const fn is_sep(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Text after the last separator.
fn basename(path: &str) -> &str {
    path.rsplit(is_sep).next().unwrap_or(path)
}

/// Text before the last separator (empty when there is none).
fn dirname(path: &str) -> &str {
    path.rfind(is_sep).map_or("", |at| &path[..at])
}

/// `fs.exists`: the path (after symlinks) can be stat'ed.
pub fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// A transcript that resolved, with the agent that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub agent: Agent,
    pub file: SessionFileRef,
}

fn strip_jsonl(s: &str) -> &str {
    s.strip_suffix(JSONL).unwrap_or(s)
}

/// A direct `.jsonl` path inside some agent's root; the first such agent
/// owns it. Its id is the file name without `.jsonl`.
fn resolve_direct_path(roots: &AgentRoots, id_or_path: &str) -> Option<Resolution> {
    if !id_or_path.ends_with(JSONL) || !exists(id_or_path) {
        return None;
    }
    let agent = Agent::ALL.into_iter().find(|&a| {
        roots
            .of(a)
            .iter()
            .any(|s| id_or_path.starts_with(&s.projects_root))
    })?;
    Some(Resolution {
        agent,
        file: SessionFileRef {
            id: SessionId::new(strip_jsonl(basename(id_or_path))),
            slug: ProjectSlug::new(""),
            path: id_or_path.to_owned(),
            stamp: crate::model::FileStamp::default(),
            source: None,
        },
    })
}

/// An exact id, first agent first, else a prefix that names exactly one
/// session id across all agents (the first copy when it is in two roots).
fn scan_for_id(roots: &AgentRoots, wanted: &str) -> Result<Resolution, SessionError> {
    let mut prefixed = Vec::new();
    for agent in Agent::ALL {
        let refs = session_files(agent, roots, None);
        if let Some(exact) = refs.iter().find(|r| r.id.as_str() == wanted) {
            return Ok(Resolution {
                agent,
                file: exact.clone(),
            });
        }
        prefixed.extend(
            refs.into_iter()
                .filter(|r| r.id.as_str().starts_with(wanted))
                .map(|file| Resolution { agent, file }),
        );
    }
    let mut seen = std::collections::HashSet::new();
    prefixed.retain(|r| seen.insert(r.file.id.clone()));
    match <[Resolution; 1]>::try_from(prefixed) {
        Ok([only]) => Ok(only),
        Err(none) if none.is_empty() => Err(SessionError::NotFound {
            id: wanted.to_owned(),
            searched: "all agents".to_owned(),
        }),
        Err(many) => Err(SessionError::ambiguous(
            wanted,
            many.into_iter()
                .map(|r| r.file.id.as_str().to_owned())
                .collect(),
        )),
    }
}

/// Resolve an id (or direct `.jsonl` path) to its owning agent and file.
pub fn resolve_session(roots: &AgentRoots, id_or_path: &str) -> Result<Resolution, SessionError> {
    let _span = info_span!("Sessions.resolve", idOrPath = id_or_path).entered();
    if let Some(r) = resolve_direct_path(roots, id_or_path) {
        return Ok(r);
    }
    scan_for_id(roots, strip_jsonl(id_or_path))
}

/// A Claude transcript's location and its subagents dir, if present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeLocation {
    pub session_id: String,
    pub project_dir: String,
    pub subagent_dir: Option<String>,
}

fn with_subagent_dir(session_id: &str, project_dir: &str) -> ClaudeLocation {
    let dir = slash_join(&[project_dir, session_id, "subagents"]);
    ClaudeLocation {
        session_id: session_id.to_owned(),
        project_dir: project_dir.to_owned(),
        subagent_dir: exists(&dir).then_some(dir),
    }
}

/// Names in `dir`, in directory order; a read error yields none.
fn read_names(dir: &str) -> Vec<String> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Slug dirs across every source, distinct, in source then directory order.
pub(crate) fn project_slugs(claude: &[Source]) -> Vec<String> {
    let _span = info_span!("AgentRegistry.listProjectSlugs", agent = "claude").entered();
    let mut out: Vec<String> = Vec::new();
    for source in claude.iter().filter(|s| exists(&s.projects_root)) {
        for name in read_names(&source.projects_root) {
            let path = crate::paths::PathStyle::NATIVE.join(&[&source.projects_root, &name]);
            if fs::metadata(&path).is_ok_and(|m| m.is_dir()) && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Re-resolve a Claude session among Claude roots only, to find its
/// project dir: a direct path, an exact id, or a unique prefix.
pub fn resolve_claude_session(
    claude: &[Source],
    id_or_path: &str,
) -> Result<ClaudeLocation, SessionError> {
    let _span = info_span!("Sessions.resolve", idOrPath = id_or_path).entered();
    if id_or_path.ends_with(JSONL) && exists(id_or_path) {
        let session_id = strip_jsonl(basename(id_or_path));
        return Ok(with_subagent_dir(session_id, dirname(id_or_path)));
    }
    let id = strip_jsonl(id_or_path);
    let slugs = project_slugs(claude);
    for source in claude {
        for slug in &slugs {
            let dir = slash_join(&[&source.projects_root, slug]);
            if exists(&slash_join(&[&dir, &format!("{id}{JSONL}")])) {
                return Ok(with_subagent_dir(id, &dir));
            }
        }
    }
    let mut matches = Vec::new();
    for source in claude {
        for slug in &slugs {
            let dir = slash_join(&[&source.projects_root, slug]);
            for name in read_names(&dir) {
                if let Some(sid) = name.strip_suffix(JSONL)
                    && sid.starts_with(id)
                {
                    matches.push((sid.to_owned(), dir.clone()));
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    matches.retain(|(sid, _)| seen.insert(sid.clone()));
    match matches.as_slice() {
        [(sid, dir)] => Ok(with_subagent_dir(sid, dir)),
        [] => Err(SessionError::NotFound {
            id: id.to_owned(),
            searched: default_root(claude),
        }),
        _ => Err(SessionError::ambiguous(
            id,
            matches.into_iter().map(|(sid, _)| sid).collect(),
        )),
    }
}

/// A subagent transcript found on disk, before its turns are parsed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubagentStub {
    pub id: String,
    pub path: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub tool_use_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub workflow_run_id: Option<String>,
    pub workflow_phase: Option<String>,
    pub workflow_phase_index: Option<f64>,
    /// Workflow agent start, epoch ms; picks the call among resumes.
    pub workflow_started_at: Option<f64>,
}

/// Read a file as `readFileString` does.
pub fn read_text(path: &str) -> std::io::Result<String> {
    fs::read(path).map(decode)
}

fn read_meta(dir: &str, id: &str) -> JsValue {
    let path = slash_join(&[dir, &format!("{id}.meta.json")]);
    match read_text(&path).map(|raw| JsValue::parse(&raw)) {
        Ok(Some(v)) => v,
        Ok(None) => {
            debug!(path = %slash_join(&[dir, id]), "subagent meta unreadable");
            JsValue::Null
        }
        Err(e) => {
            debug!(path = %slash_join(&[dir, id]), reason = %e, "subagent meta unreadable");
            JsValue::Null
        }
    }
}

fn build_stub(id: &str, path: String, meta: &JsValue, run: Option<&WorkflowRun>) -> SubagentStub {
    let field = |k: &str| meta.get(k).and_then(JsValue::as_str).map(str::to_owned);
    let mut stub = SubagentStub {
        id: id.to_owned(),
        path,
        agent_type: field("agentType"),
        description: field("description"),
        tool_use_id: field("toolUseId"),
        parent_agent_id: field("parentAgentId").map(|p| to_agent_ref(&p)),
        ..SubagentStub::default()
    };
    if let Some(run) = run {
        let phase = run.agents.get(id);
        let title = field("workflowPhase").or_else(|| phase.and_then(|p| p.phase_title.clone()));
        stub.workflow_run_id = Some(run.run_id.clone());
        stub.workflow_phase = title.filter(|t| !t.is_empty());
        stub.workflow_phase_index = phase.and_then(|p| p.phase_index);
        stub.workflow_started_at = phase.and_then(|p| p.started_at);
    }
    stub
}

pub(crate) fn is_subagent_file(name: &str) -> bool {
    name.starts_with("agent-") && name.ends_with(JSONL)
}

struct Walker<'a> {
    subagent_dir: &'a str,
    workflows_dir: String,
}

impl Walker<'_> {
    fn read_run(&self, run_id: &str) -> WorkflowRun {
        let path = slash_join(&[
            dirname(self.subagent_dir),
            "workflows",
            &format!("{run_id}.json"),
        ]);
        match read_text(&path) {
            Ok(raw) => parse_workflow_run(run_id, &raw),
            Err(e) => {
                debug!(path = run_id, reason = %e, "workflow run unreadable");
                WorkflowRun {
                    run_id: run_id.to_owned(),
                    ..WorkflowRun::default()
                }
            }
        }
    }

    fn walk(&self, dir: &str, run: Option<&WorkflowRun>, out: &mut Vec<SubagentStub>) {
        let names = match fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            Err(e) => {
                debug!(path = dir, reason = %e, "subagent dir unreadable");
                return;
            }
        };
        for name in names {
            let path = slash_join(&[dir, &name]);
            let meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    debug!(path = %path, reason = %e, "subagent entry unreadable");
                    continue;
                }
            };
            if meta.is_dir() {
                if dir == self.workflows_dir && name.starts_with(WORKFLOW_RUN) {
                    let run = self.read_run(&name);
                    self.walk(&path, Some(&run), out);
                } else {
                    self.walk(&path, run, out);
                }
            } else if meta.is_file() && is_subagent_file(&name) {
                let id = strip_jsonl(&name);
                let meta = read_meta(dir, id);
                out.push(build_stub(id, path, &meta, run));
            }
        }
    }
}

/// Every `agent-*.jsonl` under a subagents dir, in directory order.
pub fn find_subagents(subagent_dir: Option<&str>) -> Vec<SubagentStub> {
    let Some(dir) = subagent_dir.filter(|d| exists(d)) else {
        return Vec::new();
    };
    let walker = Walker {
        subagent_dir: dir,
        workflows_dir: slash_join(&[dir, "workflows"]),
    };
    let mut out = Vec::new();
    walker.walk(dir, None, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_like_ts() {
        assert_eq!(slash_join(&["/a//", "/b", "c"]), "/a/b/c");
        assert_eq!(dirname("/a/b"), "/a");
        assert_eq!(basename("/a/b.jsonl"), "b.jsonl");
    }

    #[test]
    fn not_found_names_what_was_searched() {
        assert_eq!(
            resolve_session(&AgentRoots::default(), "zzz.jsonl")
                .err()
                .map(|e| e.to_string())
                .as_deref(),
            Some("Session \"zzz\" not found (searched all agents).")
        );
    }

    #[test]
    fn ambiguity_lists_sorted_candidates_and_counts_the_rest() {
        let ids = (0..12).rev().map(|i| format!("ab{i:02}")).collect();
        let e = SessionError::ambiguous("ab", ids);
        assert_eq!(
            e.to_string(),
            "Session \"ab\" is ambiguous: it matches 12 sessions (ab00, ab01, ab02, ab03, ab04, ab05, ab06, ab07, ab08, ab09, and 2 more). Give more of the id."
        );
    }

    #[test]
    fn splits_the_drill_form_only() {
        assert_eq!(
            split_nested_id("518d7c58/agent-abe3f92a"),
            Some(("518d7c58", "agent-abe3f92a"))
        );
        assert_eq!(split_nested_id("/tmp/x/agent-a.jsonl"), None);
        assert_eq!(split_nested_id("/agent-a"), None);
        assert_eq!(split_nested_id("p/a/agent-a"), None);
        assert_eq!(split_nested_id("p/notes"), None);
        assert_eq!(split_nested_id("plain"), None);
    }
}
