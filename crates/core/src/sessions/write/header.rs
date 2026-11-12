//! Session list rows as JSON, in the key order `sessions ls --json` prints.

use crate::js::json_out::Writer;
use crate::model::{Agent, SessionHeader, SessionListing};

/// One header object at the writer's position. Pi puts `startedAt` before
/// `title`; a Codex project comes from the cwd, so it is transcript text.
pub fn write_header(w: &mut Writer, h: &SessionHeader) {
    let opt_text = |w: &mut Writer, key: &str, v: Option<&str>| {
        if let Some(v) = v {
            w.key(key).text(v);
        }
    };
    w.begin_obj();
    w.key("id").str(h.id.as_str());
    w.key("agent").str(h.agent.as_str());
    w.key("path").str(&h.path);
    if h.agent == Agent::Codex {
        w.key("project").text(h.project.as_str());
    } else {
        w.key("project").str(h.project.as_str());
    }
    #[expect(clippy::cast_possible_wrap, reason = "counts and sizes fit i64")]
    let int = |n: u64| n as i64;
    w.key("messageCount").int(int(h.message_count));
    w.key("sizeBytes").int(int(h.size_bytes));
    w.key("updatedAt").text(&h.updated_at);
    opt_text(w, "cwd", h.cwd.as_deref());
    opt_text(w, "gitBranch", h.git_branch.as_deref());
    opt_text(w, "model", h.model.as_deref());
    let (title, started) = (h.title.as_deref(), h.started_at.as_deref());
    if h.agent == Agent::Pi {
        opt_text(w, "startedAt", started);
        opt_text(w, "title", title);
    } else {
        opt_text(w, "title", title);
        opt_text(w, "startedAt", started);
    }
    if let Some(n) = h.malformed_lines {
        w.key("malformedLines").int(int(n));
    }
    if let Some(c) = h.cost_usd {
        w.key("costUsd").num(c);
    }
    if let Some(m) = &h.unpriced_models {
        w.key("unpricedModels").texts(m.iter().map(String::as_str));
    }
    if let Some(s) = &h.source {
        w.key("source").text(&s.id);
        w.key("sourceLabel").text(&s.label);
    }
    w.end_obj();
}

/// `JSON.stringify(rows)` or `JSON.stringify(rows, null, 2)`.
pub fn headers_json<'a>(rows: impl IntoIterator<Item = &'a SessionHeader>, pretty: bool) -> String {
    let mut w = Writer::new(pretty);
    w.begin_arr();
    rows.into_iter().for_each(|h| write_header(w.elem(), h));
    w.end_arr();
    w.finish()
}

/// `{headers, unreadable}`, compact.
pub fn listing_json(l: &SessionListing) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    w.key("headers").begin_arr();
    l.headers.iter().for_each(|h| write_header(w.elem(), h));
    w.end_arr();
    w.key("unreadable").begin_arr();
    for u in &l.unreadable {
        w.elem().begin_obj();
        w.key("agent").str(u.agent.as_str());
        w.key("project").text(&u.project);
        w.key("path").text(&u.path);
        w.end_obj();
    }
    w.end_arr();
    w.end_obj();
    w.finish()
}
