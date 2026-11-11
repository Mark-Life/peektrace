//! List headers per transcript file: what each listed file yields, and the
//! loader shared by agents whose header folds the whole parsed text.

use rayon::prelude::*;
use tracing::{Span, debug, field, info_span};

use crate::model::{Agent, SessionFileRef, SessionHeader, UnreadableSession};
use crate::paths::PathStyle;
use crate::sessions::resolve::read_text;

/// What one listed transcript yields.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Header(Box<SessionHeader>),
    Skip,
    Unreadable(UnreadableSession),
}

/// A transcript that failed to read. Its project is the slug's last segment.
pub(crate) fn unreadable(agent: Agent, r: &SessionFileRef, reason: &std::io::Error) -> Outcome {
    debug!(path = %r.path, reason = %reason, "session unreadable");
    Outcome::Unreadable(UnreadableSession {
        agent,
        project: PathStyle::NATIVE.basename(r.slug.as_str()).to_owned(),
        path: r.path.clone(),
    })
}

/// Log a header's malformed lines and wrap it.
pub(crate) fn header(h: Box<SessionHeader>) -> Outcome {
    if let Some(n) = h.malformed_lines {
        debug!(path = %h.path, malformedLines = n, "session has malformed lines");
    }
    Outcome::Header(h)
}

/// Builds a header from a transcript's whole text.
pub(crate) type BuildHeader = fn(&str, &SessionFileRef) -> SessionHeader;

/// Read one transcript and fold it with `build`. An empty file is skipped.
fn text_header(agent: Agent, r: &SessionFileRef, build: BuildHeader, parent: &Span) -> Outcome {
    let span = info_span!(
        parent: parent,
        "AgentRegistry.loadTranscript",
        agent = agent.as_str(),
        error = field::Empty
    );
    let _entered = span.enter();
    match read_text(&r.path) {
        Ok(text) if text.is_empty() && r.stamp.size_bytes == 0 => Outcome::Skip,
        Ok(text) => {
            let mut h = build(&text, r);
            h.source.clone_from(&r.source);
            header(Box::new(h))
        }
        Err(e) => {
            span.record("error", field::display(&e));
            unreadable(agent, r, &e)
        }
    }
}

/// One outcome per transcript, in `refs` order, read in parallel.
pub(crate) fn load_text_headers(
    agent: Agent,
    refs: &[SessionFileRef],
    build: BuildHeader,
    parent: &Span,
) -> Vec<Outcome> {
    refs.par_iter()
        .map(|r| text_header(agent, r, build, parent))
        .collect()
}
