//! List headers per transcript file: what each listed file yields, and the
//! streaming loader shared by agents whose header folds parsed rows.

use rayon::prelude::*;
use tracing::{Span, debug, field, info_span};

use crate::js::value::JsValue;
use crate::model::{Agent, SessionFileRef, SessionHeader, UnreadableSession};
use crate::paths::PathStyle;
use crate::sessions::jsonl::{scan_file, scan_text};

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

/// A list header folded from a transcript's rows, one row at a time.
pub(crate) trait RowFold: Default {
    fn row(&mut self, row: JsValue);
    fn header(self, r: &SessionFileRef, malformed: u64) -> SessionHeader;
}

/// Fold decoded text into a header.
pub(crate) fn fold_text<F: RowFold>(text: &str, r: &SessionFileRef) -> SessionHeader {
    let mut acc = F::default();
    let malformed = scan_text(text, |row| acc.row(row));
    acc.header(r, malformed)
}

/// Stream one transcript and fold it. An empty file is skipped.
fn file_header<F: RowFold>(agent: Agent, r: &SessionFileRef, parent: &Span) -> Outcome {
    let span = info_span!(
        parent: parent,
        "AgentRegistry.loadTranscript",
        agent = agent.as_str(),
        error = field::Empty
    );
    let _entered = span.enter();
    let mut acc = F::default();
    match scan_file(&r.path, |row| acc.row(row)) {
        Ok(s) if s.empty && r.stamp.size_bytes == 0 => Outcome::Skip,
        Ok(s) => {
            let mut h = acc.header(r, s.malformed);
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
pub(crate) fn stream_headers<F: RowFold>(
    agent: Agent,
    refs: &[SessionFileRef],
    parent: &Span,
) -> Vec<Outcome> {
    refs.par_iter()
        .map(|r| file_header::<F>(agent, r, parent))
        .collect()
}
