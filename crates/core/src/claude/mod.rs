//! Claude Code transcripts: discovery, line decoding and header folding.

mod attachments;
mod blocks;
mod branches;
mod header;
mod line;
pub mod parse;
mod read;
mod records;

use std::path::Path;

use rayon::prelude::*;
use tracing::{Span, field, info_span};

use header::FileMeta;
pub use parse::{ParseArgs, SubagentTurns, parse_claude_session, parse_subagent_turns};
use read::{Loaded, load_file};

use crate::discover;
pub use crate::headers::Outcome;
use crate::headers::{header, unreadable};
use crate::model::{Agent, SessionFileRef};
use crate::roots::Source;

/// Every Claude transcript across `sources`: `<root>/<slug>/<id>.jsonl`,
/// limited to the `project` slug when one is given. Subagent files under
/// `<slug>/<id>/subagents/` are never listed.
pub fn list_session_files(sources: &[Source], project: Option<&str>) -> Vec<SessionFileRef> {
    discover::list_sources(sources, |root| {
        discover::slug_layout(root, project, discover::stem)
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
        Ok(Loaded::Header(h)) => header(h),
        Ok(Loaded::Skip) => Outcome::Skip,
        Err(e) => {
            span.record("error", field::display(&e));
            unreadable(Agent::Claude, r, &e)
        }
    }
}

/// One outcome per transcript, in `refs` order. Files are read in parallel.
pub fn load_headers(refs: &[SessionFileRef], parent: &Span) -> Vec<Outcome> {
    refs.par_iter().map(|r| header_for(r, parent)).collect()
}
