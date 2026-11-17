//! Session listing across agents: one header per readable transcript, plus
//! the transcripts that failed to read.

use tracing::{Span, field, info_span};

use crate::header_cache::HeaderCache;
use crate::headers::Outcome;
use crate::model::{Agent, SessionFileRef, SessionListing};
use crate::roots::{self, RootsEnv, Settings, Source};
use crate::worktree::row_project;
use crate::{claude, codex, opencode, pi};

/// Where each agent's transcripts live.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRoots {
    pub claude: Vec<Source>,
    pub codex: Vec<Source>,
    pub pi: Vec<Source>,
    pub opencode: Vec<Source>,
    /// t3code's state dir, read for thread titles; `None` reads nothing.
    pub t3code: Option<String>,
}

impl AgentRoots {
    /// Every agent's roots from the environment and `settings.json`.
    pub fn resolve(env: &RootsEnv, settings: &Settings) -> Self {
        let of = |agent| roots::sources(agent, env, settings.extras(agent));
        Self {
            claude: of(Agent::Claude),
            codex: of(Agent::Codex),
            pi: of(Agent::Pi),
            opencode: of(Agent::Opencode),
            t3code: Some(roots::t3code_state_dir(env)),
        }
    }

    /// One agent's roots.
    pub fn of(&self, agent: Agent) -> &[Source] {
        match agent {
            Agent::Claude => &self.claude,
            Agent::Codex => &self.codex,
            Agent::Pi => &self.pi,
            Agent::Opencode => &self.opencode,
        }
    }
}

/// Server-side listing filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub agent: Option<Agent>,
    /// Exact project slug; empty means no filter.
    pub project: Option<String>,
    /// Add Claude subagent files to the cost, for callers that show it.
    /// Without it they are not read, and a cost they could add to is left
    /// unknown until [`crate::claude::add_subagent_costs_to`].
    pub with_subagent_costs: bool,
}

/// Transcript files of one agent. A Claude or Pi row's project is its slug,
/// so `project` drops other slug dirs before any of them is read. A Codex or
/// `OpenCode` row's project comes from its text, so every session is listed.
pub fn session_files(
    agent: Agent,
    roots: &AgentRoots,
    project: Option<&str>,
) -> Vec<SessionFileRef> {
    let _span = info_span!("AgentRegistry.listSessionFiles", agent = agent.as_str()).entered();
    match agent {
        Agent::Claude => claude::list_session_files(&roots.claude, project),
        Agent::Codex => codex::list_session_files(&roots.codex),
        Agent::Pi => pi::list_session_files(&roots.pi, project),
        Agent::Opencode => opencode::list_session_files(&roots.opencode),
    }
}

/// The first (default) source's root.
pub(crate) fn default_root(sources: &[Source]) -> String {
    sources
        .first()
        .map(|s| s.projects_root.clone())
        .unwrap_or_default()
}

/// Keep only what `--project` asks for: headers whose own project or
/// printed project ([`row_project`], so worktree rows count for their main
/// checkout too) is `project`, and unreadable sessions of that project or of an unknown one
/// (`""`). Runs after worktrees are resolved.
fn keep_project(listing: &mut SessionListing, project: Option<&str>) {
    let Some(p) = project.filter(|p| !p.is_empty()) else {
        return;
    };
    listing
        .headers
        .retain(|h| h.project.as_str() == p || row_project(h) == p);
    listing
        .unreadable
        .retain(|u| u.project.is_empty() || u.project == p);
}

/// Outcomes for one agent's transcripts, in discovery order. File-backed
/// agents reuse `cache` hits; `OpenCode` lives in `SQLite` and is always read.
fn agent_outcomes(
    agent: Agent,
    roots: &AgentRoots,
    filter: &ListFilter,
    cache: Option<&HeaderCache>,
    parent: &Span,
) -> Vec<Outcome> {
    let project = filter.project.as_deref();
    let refs = session_files(agent, roots, project);
    let load = |refs: &[SessionFileRef]| match agent {
        Agent::Claude => claude::load_headers(refs, parent),
        Agent::Codex => codex::load_headers(refs, parent),
        Agent::Pi => pi::load_headers(refs, parent),
        Agent::Opencode => opencode::load_headers(refs, &default_root(&roots.opencode), parent),
    };
    let complete = agent == Agent::Codex || project.is_none_or(str::is_empty);
    let mut outcomes = match cache {
        Some(c) if agent != Agent::Opencode => c.outcomes(agent, &refs, complete, load),
        _ => load(&refs),
    };
    if agent == Agent::Claude {
        if filter.with_subagent_costs {
            claude::add_subagent_costs(&mut outcomes, cache.map(|c| &c.subagents), complete);
        } else {
            claude::drop_pending_costs(&mut outcomes);
        }
    }
    outcomes
}

/// Split outcomes into the listing; empty files drop out.
fn to_listing(outcomes: Vec<Outcome>) -> SessionListing {
    outcomes
        .into_iter()
        .fold(SessionListing::default(), |mut listing, o| {
            match o {
                Outcome::Header(h) => listing.headers.push(*h),
                Outcome::Unreadable(u) => listing.unreadable.push(u),
                Outcome::Skip => {}
            }
            listing
        })
}

/// Headers for every agent (or the filtered one), only from the `project`
/// when one is set.
pub fn list_sessions(roots: &AgentRoots, filter: &ListFilter) -> SessionListing {
    list_with(roots, filter, None)
}

/// [`list_sessions`], reusing the headers of files unchanged since an
/// earlier call with the same `cache`.
pub fn list_sessions_cached(
    roots: &AgentRoots,
    filter: &ListFilter,
    cache: &HeaderCache,
) -> SessionListing {
    list_with(roots, filter, Some(cache))
}

fn list_with(
    roots: &AgentRoots,
    filter: &ListFilter,
    cache: Option<&HeaderCache>,
) -> SessionListing {
    let span = info_span!(
        "Sessions.list",
        agent = field::Empty,
        malformedLines = field::Empty
    );
    if let Some(agent) = filter.agent {
        span.record("agent", agent.as_str());
    }
    let _entered = span.enter();
    let outcomes = Agent::ALL
        .into_iter()
        .filter(|a| filter.agent.is_none_or(|f| f == *a))
        .flat_map(|agent| agent_outcomes(agent, roots, filter, cache, &span))
        .collect();
    let mut listing = to_listing(outcomes);
    crate::t3code::apply_headers(&mut listing.headers, roots.t3code.as_deref());
    keep_project(&mut listing, filter.project.as_deref());
    let malformed: u64 = listing
        .headers
        .iter()
        .filter_map(|h| h.malformed_lines)
        .sum();
    span.record("malformedLines", malformed);
    listing
}
