//! `OpenAI` Codex CLI rollouts: discovery under the date tree, list headers
//! and the full parse.

mod context;
mod header;
mod parse;

use std::sync::LazyLock;

use regex::Regex;
use tracing::Span;

pub use header::build_header;
pub use parse::parse_codex_session;

use crate::discover::{self, JSONL};
use crate::headers::{Outcome, load_text_headers};
use crate::model::{Agent, SessionFileRef};
use crate::roots::Source;

/// Any RFC 4122 uuid, as embedded in rollout and Pi file names.
pub(crate) static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})")
        .unwrap_or_else(|e| unreachable!("bad pattern: {e}"))
});

/// `^rollout-.*\.jsonl$`, where `.` is any char but a line terminator.
fn is_rollout(name: &str) -> bool {
    name.strip_prefix("rollout-")
        .and_then(|rest| rest.strip_suffix(JSONL))
        .is_some_and(|mid| !mid.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
}

/// The uuid in a rollout's name, else the name without `.jsonl`.
fn codex_id(name: &str) -> String {
    UUID.find(name)
        .map_or_else(|| discover::stem(name), |m| m.as_str().to_owned())
}

/// Every `rollout-*.jsonl` under each source root, at any depth.
pub fn list_session_files(sources: &[Source]) -> Vec<SessionFileRef> {
    discover::list_sources(sources, |root| {
        discover::walk_tree(root, is_rollout, codex_id)
    })
}

/// One header per rollout, in `refs` order.
pub fn load_headers(refs: &[SessionFileRef], parent: &Span) -> Vec<Outcome> {
    load_text_headers(Agent::Codex, refs, build_header, parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names() {
        assert_eq!(
            codex_id("rollout-2026-01-02T10-00-00-019F42F4-342d-7b30-9551-a78e5a2cbe5e.jsonl"),
            "019F42F4-342d-7b30-9551-a78e5a2cbe5e"
        );
        assert_eq!(codex_id("rollout-demo.jsonl"), "rollout-demo");
        assert!(is_rollout("rollout-.jsonl"));
        assert!(!is_rollout("rollout.jsonl"));
        assert!(!is_rollout("notrollout-a.jsonl"));
        assert!(!is_rollout("rollout-a\n.jsonl"));
    }
}
