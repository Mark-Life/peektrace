//! One session end to end: resolve it, read it, parse it with its agent's
//! parser, fold in Claude subagents, then analyze or redact.

use std::collections::HashMap;

use rayon::prelude::*;
use tracing::{debug, field, info_span};

use super::analyze::{AnalyzeOptions, analyze};
use super::context_files::gather_on_disk_context_files;
use super::cost::turns_cost;
use super::model::{AnalyzedSession, ParsedSession, SubagentRef, TurnsCost};
use super::redact::{redact_parsed, redact_session};
use super::resolve::{
    SessionError, SubagentStub, find_subagents, read_text, resolve_claude_session, resolve_session,
};
use super::workflows::{WorkflowCall, pick_workflow_call, workflow_calls_by_run};
use crate::claude::{ParseArgs, parse_claude_session, parse_subagent_turns};
use crate::codex::parse_codex_session;
use crate::js::time::{self, LocalZone};
use crate::js::value::JsValue;
use crate::listing::AgentRoots;
use crate::model::{Agent, SessionHeader};
use crate::pi::parse_pi_session;

/// What lookups and parses read from the environment.
#[derive(Clone, Copy)]
pub struct SessionEnv<'a> {
    pub roots: &'a AgentRoots,
    /// Agents with sessions on disk that this build cannot read.
    pub unsearched: &'a [Agent],
    /// Zone for timestamps without an offset.
    pub local: &'a (dyn LocalZone + Sync),
}

/// The session a command runs on.
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    /// An id, unique id prefix or `.jsonl` path to look up.
    Lookup(&'a str),
    /// A session a listing already found, read without another lookup.
    Listed(&'a SessionHeader),
}

impl Target<'_> {
    /// The typed id or path, or the listed session's id.
    pub fn id(&self) -> &str {
        match self {
            Self::Lookup(id) => id,
            Self::Listed(h) => h.id.as_str(),
        }
    }
}

/// A transcript read through its agent.
struct Loaded {
    agent: Agent,
    id: String,
    nested: bool,
    path: String,
    source: Option<String>,
    text: String,
}

/// `String(e)` of the platform error a failed read raises.
fn read_reason(path: &str, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!(
            "SystemError: PermissionDenied: FileSystem.readFile ({path}): EACCES: permission denied, open '{path}'"
        ),
        std::io::ErrorKind::NotFound => format!(
            "SystemError: NotFound: FileSystem.stat ({path}): ENOENT: no such file or directory, stat '{path}'"
        ),
        _ => format!("SystemError: Unknown: FileSystem.readFile ({path}): {e}"),
    }
}

fn load_transcript(agent: Agent, path: &str) -> Result<String, SessionError> {
    let span = info_span!(
        "AgentRegistry.loadTranscript",
        agent = agent.as_str(),
        error = field::Empty
    );
    let _entered = span.enter();
    read_text(path).map_err(|e| {
        span.record("error", field::display(&e));
        SessionError::Read {
            path: path.to_owned(),
            reason: read_reason(path, &e),
        }
    })
}

/// Read a subagent transcript through its parent session.
fn load_nested(env: SessionEnv<'_>, id: &str, parent: &str) -> Result<Loaded, SessionError> {
    let not_found = || SessionError::NotFound {
        id: id.to_owned(),
        searched: format!("subagents of {parent}"),
    };
    let owner = resolve_session(env.roots, env.unsearched, parent)?;
    if owner.agent != Agent::Claude {
        return Err(not_found());
    }
    let loc = resolve_claude_session(&env.roots.claude, &owner.file.path)?;
    let stubs = find_subagents(loc.subagent_dir.as_deref());
    let mut prefixed = stubs.iter().filter(|s| s.id.starts_with(id));
    let unique = match (prefixed.next(), prefixed.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    };
    let stub = stubs
        .iter()
        .find(|s| s.id == id)
        .or(unique)
        .ok_or_else(not_found)?;
    Ok(Loaded {
        agent: Agent::Claude,
        id: stub.id.clone(),
        nested: true,
        path: stub.path.clone(),
        source: owner.file.source.map(|s| s.id),
        text: load_transcript(Agent::Claude, &stub.path)?,
    })
}

fn load(
    env: SessionEnv<'_>,
    target: Target<'_>,
    parent: Option<&str>,
) -> Result<Loaded, SessionError> {
    if let Some(parent) = parent {
        return load_nested(env, target.id(), parent);
    }
    let (agent, id, path, source) = match target {
        Target::Lookup(id) => {
            let r = resolve_session(env.roots, env.unsearched, id)?;
            let source = r.file.source.map(|s| s.id);
            (r.agent, r.file.id.as_str().to_owned(), r.file.path, source)
        }
        Target::Listed(h) => {
            let _span = info_span!("Sessions.resolve", idOrPath = h.id.as_str()).entered();
            let source = h.source.as_ref().map(|s| s.id.clone());
            (h.agent, h.id.as_str().to_owned(), h.path.clone(), source)
        }
    };
    Ok(Loaded {
        agent,
        text: load_transcript(agent, &path)?,
        id,
        nested: false,
        path,
        source,
    })
}

/// Points each workflow subagent's `toolUseId` at the Workflow call that
/// started its run (or the resume live when it started).
struct Linker {
    calls: HashMap<String, Vec<WorkflowCall>>,
}

impl Linker {
    fn new(stubs: &[SubagentStub], text: &str, local: &dyn LocalZone) -> Self {
        let wants = stubs
            .iter()
            .any(|s| s.workflow_run_id.is_some() && s.tool_use_id.is_none());
        Self {
            calls: if wants {
                workflow_calls_by_run(text, local)
            } else {
                HashMap::default()
            },
        }
    }

    /// The stub as a ref with its link, before its turns are known.
    fn link(
        &self,
        stub: &SubagentStub,
        started_at: Option<&str>,
        local: &dyn LocalZone,
    ) -> SubagentRef {
        let mut out = SubagentRef {
            id: stub.id.clone(),
            path: stub.path.clone(),
            agent_type: stub.agent_type.clone(),
            description: stub.description.clone(),
            tool_use_id: stub.tool_use_id.clone(),
            parent_agent_id: stub.parent_agent_id.clone(),
            workflow_run_id: stub.workflow_run_id.clone(),
            workflow_phase: stub.workflow_phase.clone(),
            workflow_phase_index: stub.workflow_phase_index,
            tool_use_id_linked: false,
            turns: 0,
            peak_context_tokens: 0.0,
            cost: TurnsCost::default(),
            unreadable: false,
        };
        let Some(run) = stub.workflow_run_id.as_deref().filter(|r| !r.is_empty()) else {
            return out;
        };
        if stub.tool_use_id.is_some() {
            return out;
        }
        #[expect(clippy::cast_precision_loss, reason = "dates are within 8.64e15")]
        let fallback = started_at
            .filter(|s| !s.is_empty())
            .and_then(|s| time::parse_date(s, local))
            .map(|ms| ms as f64);
        let calls = self.calls.get(run).map(Vec::as_slice).unwrap_or_default();
        if let Some(call) = pick_workflow_call(calls, stub.workflow_started_at.or(fallback)) {
            out.tool_use_id = Some(call.to_owned());
            out.tool_use_id_linked = true;
        }
        out
    }
}

/// One subagent file as its ref: its own turns, peak and cost.
fn parse_subagent(stub: &SubagentStub, linker: &Linker, local: &dyn LocalZone) -> SubagentRef {
    match read_text(&stub.path) {
        Ok(text) => {
            let parsed = parse_subagent_turns(&text);
            SubagentRef {
                turns: parsed.turns.len(),
                peak_context_tokens: parsed
                    .turns
                    .iter()
                    .fold(0.0, |m: f64, t| m.max(t.context_tokens)),
                cost: turns_cost(&parsed.turns),
                ..linker.link(stub, parsed.started_at.as_deref(), local)
            }
        }
        Err(e) => {
            debug!(path = %stub.path, reason = %e, "subagent unreadable");
            SubagentRef {
                unreadable: true,
                ..linker.link(stub, None, local)
            }
        }
    }
}

/// Fold Claude subagent transcripts into a parsed session. A failure to
/// locate them keeps the session without subagents.
fn fold_claude_subagents(env: SessionEnv<'_>, parsed: &mut ParsedSession, id: &str, text: &str) {
    let loc = match resolve_claude_session(&env.roots.claude, id) {
        Ok(loc) => loc,
        Err(e) => {
            debug!(id, reason = %e, "subagents not folded");
            return;
        }
    };
    let stubs = find_subagents(loc.subagent_dir.as_deref());
    let linker = Linker::new(&stubs, text, env.local);
    parsed.subagents = stubs
        .par_iter()
        .map(|s| parse_subagent(s, &linker, env.local))
        .collect();
}

/// A full parse and where it came from.
pub struct FullParse {
    pub agent: Agent,
    pub parsed: ParsedSession,
    /// Id of the Claude root that owns the session, when several are set.
    pub source: Option<String>,
}

/// Resolve and fully parse one transcript (`parent` names the session a
/// subagent transcript sits under).
pub fn parse_full(
    env: SessionEnv<'_>,
    target: Target<'_>,
    parent: Option<&str>,
) -> Result<FullParse, SessionError> {
    let id = target.id();
    let span = match parent {
        None => info_span!("Sessions.parse", id, error = field::Empty),
        Some(p) => info_span!("Sessions.parseNested", id, parent = p, error = field::Empty),
    };
    let _entered = span.enter();
    let result = load(env, target, parent).map(|loaded| {
        let (text, path, sid) = (&loaded.text, &loaded.path, &loaded.id);
        let parsed = match loaded.agent {
            Agent::Codex => parse_codex_session(text, path, sid),
            Agent::Pi => parse_pi_session(text, path, sid),
            Agent::Claude | Agent::Opencode => {
                let mut parsed = parse_claude_session(&ParseArgs {
                    text,
                    path,
                    session_id: sid,
                    include_sidechain_turns: loaded.nested,
                    skip_system_prompt: false,
                });
                if !loaded.nested {
                    fold_claude_subagents(env, &mut parsed, id, text);
                }
                parsed
            }
        };
        FullParse {
            agent: loaded.agent,
            parsed,
            source: loaded.source,
        }
    });
    if let Err(e) = &result {
        span.record("error", field::display(e));
    }
    result
}

/// `sessions.get`: the parsed session, redacted unless `redact` is false.
pub fn get_session(
    env: SessionEnv<'_>,
    target: Target<'_>,
    parent: Option<&str>,
    redact: bool,
) -> Result<ParsedSession, SessionError> {
    let mut parsed = parse_full(env, target, parent)?.parsed;
    if redact {
        redact_parsed(&mut parsed);
    }
    Ok(parsed)
}

/// `sessions.analyze`: context forensics, always with on-disk context files
/// for Claude, redacted unless `redact` is false.
pub fn analyze_session(
    env: SessionEnv<'_>,
    target: Target<'_>,
    parent: Option<&str>,
    redact: bool,
) -> Result<AnalyzedSession, SessionError> {
    let span = info_span!("Sessions.analyze", id = target.id(), error = field::Empty);
    let _entered = span.enter();
    let result = parse_full(env, target, parent).map(|full| {
        let files = if full.agent == Agent::Claude {
            let owning = full
                .source
                .as_deref()
                .and_then(|s| env.roots.claude.iter().find(|c| c.id == s))
                .or_else(|| env.roots.claude.first());
            let (home, root) =
                owning.map_or(("", ""), |s| (s.home.as_str(), s.projects_root.as_str()));
            gather_on_disk_context_files(full.parsed.cwd.as_deref(), home, root)
        } else {
            Vec::new()
        };
        let mut a = analyze(
            full.parsed,
            AnalyzeOptions {
                dumb_zone_fraction: None,
                on_disk_context_files: files,
                window: None,
                local: env.local,
            },
        );
        if redact {
            redact_session(&mut a);
        }
        a
    });
    if let Err(e) = &result {
        span.record("error", field::display(e));
    }
    result
}

/// One image's bytes from a Claude transcript, located by the event's line
/// and image ref; `None` when the ref names no showable image.
pub fn session_image(
    env: SessionEnv<'_>,
    id: &str,
    parent: Option<&str>,
    line: usize,
    block: usize,
    item: Option<usize>,
) -> Result<Option<super::images::ImageData>, SessionError> {
    let _span = info_span!("Sessions.image", id, line).entered();
    let loaded = load(env, Target::Lookup(id), parent)?;
    let at = |raw: Option<JsValue>| raw.and_then(|v| super::images::image_at(&v, block, item));
    Ok(at(super::jsonl::line_at(&loaded.text, line))
        .or_else(|| at(super::jsonl::parse_rows(&loaded.text).into_iter().nth(line))))
}
