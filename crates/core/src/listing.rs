//! Session listing across agents: one header per readable transcript, plus
//! the transcripts that failed to read.

use tracing::{Span, field, info_span};

use crate::claude::{self, Outcome};
use crate::model::{Agent, SessionFileRef, SessionListing};
use crate::roots::Source;

/// Where each agent's transcripts live.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRoots {
    pub claude: Vec<Source>,
}

/// Server-side listing filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub agent: Option<Agent>,
    /// Exact project slug; empty means no filter.
    pub project: Option<String>,
}

/// Transcript files of one agent. Only Claude has a reader so far; the other
/// agents list nothing.
fn session_files(agent: Agent, roots: &AgentRoots) -> Vec<SessionFileRef> {
    let _span = info_span!("AgentRegistry.listSessionFiles", agent = agent.as_str()).entered();
    match agent {
        Agent::Claude => claude::list_session_files(&roots.claude),
        Agent::Codex | Agent::Pi | Agent::Opencode => Vec::new(),
    }
}

/// Outcomes for one agent's transcripts, in discovery order.
fn agent_outcomes(agent: Agent, roots: &AgentRoots, parent: &Span) -> Vec<Outcome> {
    let refs = session_files(agent, roots);
    match agent {
        Agent::Claude => claude::load_headers(&refs, parent),
        Agent::Codex | Agent::Pi | Agent::Opencode => Vec::new(),
    }
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

/// Keep rows of `project` (unreadable rows with no project too).
fn filter_project(mut listing: SessionListing, project: Option<&str>) -> SessionListing {
    if let Some(p) = project.filter(|p| !p.is_empty()) {
        listing.headers.retain(|h| h.project.as_str() == p);
        listing
            .unreadable
            .retain(|u| u.project == p || u.project.is_empty());
    }
    listing
}

/// Headers for every agent (or the filtered one), then the project filter.
pub fn list_sessions(roots: &AgentRoots, filter: &ListFilter) -> SessionListing {
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
        .flat_map(|agent| agent_outcomes(agent, roots, &span))
        .collect();
    let listing = to_listing(outcomes);
    let malformed: u64 = listing
        .headers
        .iter()
        .filter_map(|h| h.malformed_lines)
        .sum();
    span.record("malformedLines", malformed);
    filter_project(listing, filter.project.as_deref())
}
