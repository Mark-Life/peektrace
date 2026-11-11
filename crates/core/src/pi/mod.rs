//! Pi (pi.dev) sessions: discovery under cwd slug dirs, list headers and
//! the full parse, with the active branch and context edits applied.

mod branch;
mod context;
mod edits;
mod header;
mod parse;

use tracing::Span;

pub use header::build_header;
pub use parse::parse_pi_session;

use crate::codex::UUID;
use crate::discover::{self, JSONL};
use crate::headers::{Outcome, load_text_headers};
use crate::model::{Agent, SessionFileRef};
use crate::roots::Source;

/// The uuid after the first `_` of `<iso>_<uuid>.jsonl`, else the text
/// after `_` when there is some, else the name without `.jsonl`.
fn pi_id(name: &str) -> String {
    let stem = name.strip_suffix(JSONL).unwrap_or(name);
    let after = stem.find('_').map_or(stem, |at| &stem[at + 1..]);
    UUID.find(after).map_or_else(
        || if after.is_empty() { stem } else { after }.to_owned(),
        |m| m.as_str().to_owned(),
    )
}

/// Every `<root>/<slug>/*.jsonl` across `sources`, limited to the `project`
/// slug when one is given.
pub fn list_session_files(sources: &[Source], project: Option<&str>) -> Vec<SessionFileRef> {
    discover::list_sources(sources, |root| discover::slug_layout(root, project, pi_id))
}

/// One header per session file, in `refs` order.
pub fn load_headers(refs: &[SessionFileRef], parent: &Span) -> Vec<Outcome> {
    load_text_headers(Agent::Pi, refs, build_header, parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(
            pi_id("2026-01-02T10-00-00-000Z_019f42f4-342d-7b30-9551-a78e5a2cbe5e.jsonl"),
            "019f42f4-342d-7b30-9551-a78e5a2cbe5e"
        );
        assert_eq!(pi_id("2026_pinum.jsonl"), "pinum");
        assert_eq!(pi_id("pi-fixture.jsonl"), "pi-fixture");
        assert_eq!(pi_id("x_.jsonl"), "x_");
        assert_eq!(pi_id(".jsonl"), "");
    }
}
