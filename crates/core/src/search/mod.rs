//! Search every session's text for a phrase. Candidates come from the
//! listing (agent, project, since), newest first. Transcripts load a few at
//! a time, newest first, and the scan stops once `limit + 1` sessions
//! matched, so a common phrase in recent sessions reads little.

mod fold;
mod json;
mod matcher;
mod snippet;
mod window;

use std::time::Instant;

use tracing::{field, info_span};

pub use json::result_json;
pub use matcher::{
    MatchArgs, Matcher, Patterns, SearchHit, TranscriptMatch, match_transcript, query_patterns,
    words,
};
pub use snippet::{Snippet, build_snippet, redact_display};

use crate::export::home::HomeCollapser;
use crate::header_cache::HeaderCache;
use crate::js::time::LocalZone;
use crate::js::units;
use crate::listing::{AgentRoots, ListFilter, default_root, list_sessions, list_sessions_cached};
use crate::model::{Agent, SessionHeader};
use crate::opencode;
use crate::select::{self, SelectOptions};
use crate::sessions::service::load_transcript;

/// Default `limit` of a search.
pub const DEFAULT_LIMIT: usize = 20;
const HITS_PER_SESSION: usize = 3;

/// What to search for and where.
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    pub q: String,
    pub agent: Option<Agent>,
    /// A raw project slug, or (with `redact`) its redacted display form.
    pub project: Option<String>,
    pub since_ms: Option<f64>,
    /// Max sessions; 0 means all.
    pub limit: usize,
    pub redact: bool,
}

/// One session with hits. `title` and `project` are redacted when asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSession {
    pub id: String,
    pub agent: Agent,
    pub project: String,
    pub title: Option<String>,
    pub updated_at: String,
    /// Source id, when the agent has more than one root.
    pub source: Option<String>,
    pub match_count: usize,
    pub hits: Vec<SearchHit>,
}

/// Matching sessions, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchResult {
    pub sessions: Vec<SearchSession>,
    /// More sessions match than `limit`.
    pub truncated: bool,
    pub scanned_sessions: usize,
    /// Sessions the listing or the scan could not read.
    pub unreadable: usize,
}

/// A search result and the unfiltered listing's unreadable count, which the
/// root span records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchRun {
    pub result: SearchResult,
    pub listing_unreadable: usize,
}

/// What a search reads from the environment.
#[derive(Clone, Copy)]
pub struct SearchEnv<'a> {
    pub roots: &'a AgentRoots,
    pub home: Option<&'a str>,
    /// Zone for timestamps without an offset.
    pub local: &'a (dyn LocalZone + Sync),
    /// List headers kept between calls, when the caller has them.
    pub headers: Option<&'a HeaderCache>,
}

/// Text of one transcript as the agent loads it; `None` when it cannot be
/// read.
fn load_text(h: &SessionHeader, roots: &AgentRoots) -> Option<String> {
    match h.agent {
        Agent::Opencode => {
            opencode::load_session(&h.path, h.id.as_str(), &default_root(&roots.opencode))
                .map(|s| s.dialect_text())
        }
        agent => load_transcript(agent, &h.path).ok(),
    }
}

/// What scanning one candidate gave.
enum Scan {
    Failed,
    Read {
        units: usize,
        found: Option<SearchSession>,
    },
}

struct Scanner<'a> {
    matcher: Matcher,
    roots: &'a AgentRoots,
    redact: bool,
    collapser: HomeCollapser,
}

impl Scanner<'_> {
    fn display(&self, s: &str) -> String {
        if self.redact {
            redact_display(s, &self.collapser)
        } else {
            s.to_owned()
        }
    }

    fn scan(&self, h: &SessionHeader) -> Scan {
        let Some(text) = load_text(h, self.roots) else {
            return Scan::Failed;
        };
        let m = self
            .matcher
            .run(&text, self.redact, HITS_PER_SESSION, &self.collapser);
        let found = (m.match_count > 0).then(|| SearchSession {
            id: h.id.as_str().to_owned(),
            agent: h.agent,
            project: self.display(h.project.as_str()),
            title: h.title.as_deref().map(|t| self.display(t)),
            updated_at: h.updated_at.clone(),
            source: h.source.as_ref().map(|s| s.id.clone()),
            match_count: m.match_count,
            hits: m.hits,
        });
        Scan::Read {
            units: units::len(&text),
            found,
        }
    }
}

/// Sessions whose text holds `req.q`, newest first.
pub fn search_sessions(env: SearchEnv<'_>, req: &SearchRequest) -> SearchRun {
    if words(&req.q).is_empty() {
        return SearchRun::default();
    }
    let span = info_span!(
        "Sessions.search",
        scanned = field::Empty,
        matched = field::Empty,
        bytes = field::Empty,
        ms = field::Empty
    );
    let _entered = span.enter();
    let started = Instant::now();
    let scanner = Scanner {
        matcher: Matcher::new(&req.q),
        roots: env.roots,
        redact: req.redact,
        collapser: HomeCollapser::new(env.home),
    };
    let in_project = |name: &str| match req.project.as_deref() {
        None => true,
        Some(p) => name == p || (req.redact && scanner.display(name) == p),
    };
    let filter = ListFilter {
        agent: req.agent,
        project: None,
    };
    let listing = match env.headers {
        Some(cache) => list_sessions_cached(env.roots, &filter, cache),
        None => list_sessions(env.roots, &filter),
    };
    let listing_unreadable = listing.unreadable.len();
    let unreadable_listed = listing
        .unreadable
        .iter()
        .filter(|u| u.project.is_empty() || in_project(&u.project))
        .count();
    let headers: Vec<SessionHeader> = listing
        .headers
        .into_iter()
        .filter(|h| in_project(h.project.as_str()))
        .collect();
    let candidates = select::select_sessions(
        headers,
        &SelectOptions {
            since_ms: req.since_ms,
            cwd: None,
            limit: 0,
        },
        env.local,
    )
    .rows;
    let stop_after = (req.limit > 0).then(|| req.limit + 1);
    let scans = window::scan_in_order(candidates.len(), stop_after, |i| {
        let s = span.in_scope(|| scanner.scan(&candidates[i]));
        let hit = matches!(s, Scan::Read { found: Some(_), .. });
        (s, hit)
    });
    let mut found: Vec<SearchSession> = Vec::new();
    let (mut loaded, mut failed, mut bytes) = (0, 0, 0);
    for s in scans {
        match s {
            Scan::Failed => failed += 1,
            Scan::Read { units, found: hit } => {
                loaded += 1;
                bytes += units;
                found.extend(hit);
            }
        }
    }
    let all = found.len();
    if req.limit > 0 {
        found.truncate(req.limit);
    }
    span.record("scanned", loaded);
    span.record("matched", found.len());
    span.record("bytes", bytes);
    span.record(
        "ms",
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
    SearchRun {
        result: SearchResult {
            truncated: all > found.len(),
            sessions: found,
            scanned_sessions: loaded,
            unreadable: unreadable_listed + failed,
        },
        listing_unreadable,
    }
}
