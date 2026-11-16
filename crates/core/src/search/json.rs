//! `SessionSearchResult` JSON, for the CLI `--json` and the API.

use super::{SearchResult, SearchSession};
use crate::js::json_out::Writer;

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn session_json(w: &mut Writer, s: &SearchSession) {
    w.begin_obj();
    w.key("id").text(&s.id);
    w.key("agent").str(s.agent.as_str());
    w.key("project").text(&s.project);
    if let Some(t) = &s.title {
        w.key("title").text(t);
    }
    w.key("updatedAt").text(&s.updated_at);
    if let Some(src) = &s.source {
        w.key("source").text(src);
    }
    w.key("matchCount").int(count(s.match_count));
    w.key("hits").begin_arr();
    for h in &s.hits {
        w.elem().begin_obj();
        w.key("line").int(count(h.line));
        w.key("snippet").text(&h.snippet);
        if let (Some(a), Some(b)) = (h.match_start, h.match_end) {
            w.key("matchStart").int(count(a));
            w.key("matchEnd").int(count(b));
        }
        w.end_obj();
    }
    w.end_arr();
    w.end_obj();
}

/// `JSON.stringify(result)`, indented by 2 when `pretty`.
pub fn result_json(r: &SearchResult, pretty: bool) -> String {
    let mut w = Writer::new(pretty);
    w.begin_obj();
    w.key("sessions").begin_arr();
    for s in &r.sessions {
        w.elem();
        session_json(&mut w, s);
    }
    w.end_arr();
    w.key("truncated").bool(r.truncated);
    w.key("scannedSessions").int(count(r.scanned_sessions));
    w.key("unreadable").int(count(r.unreadable));
    w.end_obj();
    w.finish()
}
