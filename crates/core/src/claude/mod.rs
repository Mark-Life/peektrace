//! Claude Code transcripts: discovery, line decoding and header folding.

mod discover;
mod header;
mod line;
mod read;

use std::path::Path;

use rayon::prelude::*;
use tracing::{Span, debug, field, info_span};

pub use discover::list_session_files;
use header::FileMeta;
use read::{Loaded, load_file};

use crate::model::{Agent, SessionFileRef, SessionHeader, UnreadableSession};
use crate::paths::PathStyle;

/// What one listed transcript yields.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Header(Box<SessionHeader>),
    Skip,
    Unreadable(UnreadableSession),
}

fn unreadable(r: &SessionFileRef, reason: &std::io::Error) -> Outcome {
    debug!(path = %r.path, reason = %reason, "session unreadable");
    Outcome::Unreadable(UnreadableSession {
        agent: Agent::Claude,
        project: PathStyle::NATIVE.basename(r.slug.as_str()).to_owned(),
        path: r.path.clone(),
    })
}

/// Fold one transcript. A read error marks it unreadable.
fn header_for(r: &SessionFileRef, parent: &Span) -> Outcome {
    let path = Path::new(&r.path);
    let span = info_span!(
        parent: parent,
        "AgentRegistry.loadTranscript",
        agent = Agent::Claude.as_str(),
        error = field::Empty
    );
    let _entered = span.enter();
    let meta = FileMeta {
        id: r.id.clone(),
        slug: r.slug.clone(),
        path: r.path.clone(),
        size_bytes: r.stamp.size_bytes,
        mtime_ms: r.stamp.mtime_ms,
        source: r.source.clone(),
    };
    match load_file(path, meta) {
        Ok(Loaded::Header(h)) => {
            if let Some(n) = h.malformed_lines {
                debug!(path = %r.path, malformedLines = n, "session has malformed lines");
            }
            Outcome::Header(h)
        }
        Ok(Loaded::Skip) => Outcome::Skip,
        Err(e) => {
            span.record("error", field::display(&e));
            unreadable(r, &e)
        }
    }
}

/// One outcome per transcript, in `refs` order. Files are read in parallel.
pub fn load_headers(refs: &[SessionFileRef], parent: &Span) -> Vec<Outcome> {
    refs.par_iter().map(|r| header_for(r, parent)).collect()
}
