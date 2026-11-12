//! Render a parsed session as HTML, Markdown or JSON. Callers redact first
//! and pass `redacted`, so home paths are collapsed too.

mod home;
mod html;
mod markdown;
mod shared;

pub use home::HomeCollapser;
pub use html::render_html;
pub use markdown::render_markdown;
pub use shared::{ExportOptions, fence_for};

use crate::js::json_out::Writer;
use crate::sessions::model::ParsedSession;
use shared::prepare_export;

const EXPORT_SCHEMA: &str = "peektrace.session-export";
const EXPORT_VERSION: i64 = 1;

/// An export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Html,
    Md,
    Json,
}

impl ExportFormat {
    pub const ALL: [Self; 3] = [Self::Html, Self::Md, Self::Json];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Md => "md",
            Self::Json => "json",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == s)
    }
}

/// The session as JSON built from a field whitelist.
pub fn render_json(session: &ParsedSession, opts: &ExportOptions) -> String {
    let d = prepare_export(session, opts);
    let mut w = Writer::new(true);
    let opt_text = |w: &mut Writer, key: &str, v: Option<&String>| {
        if let Some(v) = v {
            w.key(key).text(v);
        }
    };
    let opt_bool = |w: &mut Writer, key: &str, v: Option<bool>| {
        if let Some(v) = v {
            w.key(key).bool(v);
        }
    };
    w.begin_obj();
    w.key("schema").str(EXPORT_SCHEMA);
    w.key("version").int(EXPORT_VERSION);
    w.key("redacted").bool(d.redacted);
    w.key("exportedAt").text(&d.exported_at);
    w.key("session").begin_obj();
    w.key("sessionId").text(&d.session_id);
    w.key("provider").str(d.provider.as_str());
    w.key("models").texts(d.models.iter().map(String::as_str));
    w.key("title").text(&d.title);
    opt_text(&mut w, "gitBranch", d.git_branch.as_ref());
    opt_text(&mut w, "cwd", d.cwd.as_ref());
    opt_text(&mut w, "startedAt", d.started_at.as_ref());
    opt_text(&mut w, "endedAt", d.ended_at.as_ref());
    w.key("events").begin_arr();
    for e in &d.events {
        w.elem().begin_obj();
        w.key("index")
            .int(i64::try_from(e.index).unwrap_or(i64::MAX));
        w.key("kind").str(e.kind.as_str());
        opt_text(&mut w, "ts", e.ts.as_ref());
        w.key("label").text(&e.label);
        w.key("title").text(&e.title);
        w.key("body").text(&e.body);
        opt_text(&mut w, "toolName", e.tool_name.as_ref());
        opt_text(&mut w, "toolUseId", e.tool_use_id.as_ref());
        opt_bool(&mut w, "isError", e.is_error);
        opt_bool(&mut w, "isMeta", e.is_meta);
        opt_bool(&mut w, "isSidechain", e.is_sidechain);
        if let Some(o) = e.prompt_origin {
            w.key("promptOrigin").str(o.as_str());
        }
        w.key("tokensEst").num(e.tokens_est);
        if let Some(m) = e.tokens_measured {
            w.key("tokensMeasured").num(m);
        }
        if let Some((status, original)) = &e.context_edit {
            w.key("contextEdit").begin_obj();
            w.key("status").str(status);
            opt_text(&mut w, "original", original.as_ref());
            w.end_obj();
        }
        w.end_obj();
    }
    w.end_arr();
    w.key("subagents").begin_arr();
    for s in &d.subagents {
        w.elem().begin_obj();
        w.key("id").text(&s.id);
        opt_text(&mut w, "agentType", s.agent_type.as_ref());
        opt_text(&mut w, "description", s.description.as_ref());
        w.key("turns")
            .int(i64::try_from(s.turns).unwrap_or(i64::MAX));
        w.key("peakContextTokens").num(s.peak_context_tokens);
        opt_text(&mut w, "workflowPhase", s.workflow_phase.as_ref());
        if s.unreadable {
            w.key("unreadable").bool(true);
        }
        w.end_obj();
    }
    w.end_arr();
    w.end_obj();
    w.end_obj();
    let mut out = w.finish();
    out.push('\n');
    out
}

/// Render a session in `format`.
pub fn render_export(
    format: ExportFormat,
    session: &ParsedSession,
    opts: &ExportOptions,
) -> String {
    match format {
        ExportFormat::Html => render_html(session, opts),
        ExportFormat::Md => render_markdown(session, opts),
        ExportFormat::Json => render_json(session, opts),
    }
}
