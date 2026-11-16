//! A session as one self-contained HTML page: inline CSS, no script, no
//! external loads, every text value escaped.

use super::shared::{
    ExportDoc, ExportEvent, ExportOptions, LABEL_MAX, NOTE_MAX, Role, TITLE_MAX, meta_rows,
    one_line, one_line_default, prepare_export, subagent_summary, tail_of, who_of,
};
use std::fmt::Write as _;

use crate::sessions::model::ParsedSession;

/// Escape text for element content and quoted attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const CSS: &str = "
:root{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--mute:#666;--line:#ddd;--card:#f6f6f6;--user:#e8f0fe;--err:#c0392b}
@media (prefers-color-scheme:dark){:root{--bg:#141414;--fg:#e8e8e8;--mute:#999;--line:#333;--card:#1e1e1e;--user:#1c2a44;--err:#ff8a80}}
body{margin:0 auto;max-width:56rem;padding:1.5rem;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}
h1{font-size:1.4rem;margin:0 0 .5rem}
dl{display:grid;grid-template-columns:max-content 1fr;gap:.15rem 1rem;color:var(--mute);margin:0 0 1.5rem}
dt{font-weight:600}dd{margin:0;overflow-wrap:anywhere}
.msg,details{border:1px solid var(--line);border-radius:6px;margin:.75rem 0;padding:.6rem .8rem;background:var(--card)}
.msg.user{background:var(--user)}
.who{font-weight:600;font-size:.85rem;color:var(--mute);margin:0 0 .3rem}
summary{cursor:pointer;color:var(--mute);font-size:.85rem}
pre{white-space:pre-wrap;overflow-wrap:anywhere;margin:.4rem 0 0;font:13px/1.45 ui-monospace,monospace}
.note{color:var(--mute);font-style:italic;font-size:.85rem}
.err summary{color:var(--err)}
";

fn meta(d: &ExportDoc) -> String {
    meta_rows(d).iter().fold(String::new(), |mut out, (k, v)| {
        let _ = write!(
            out,
            "<dt>{}</dt><dd>{}</dd>",
            escape_html(k),
            escape_html(v)
        );
        out
    })
}

fn subagents(d: &ExportDoc) -> String {
    if d.subagents.is_empty() {
        return String::new();
    }
    let items = d.subagents.iter().fold(String::new(), |mut out, s| {
        let _ = write!(
            out,
            "<li><code>{}</code>: {}</li>",
            escape_html(&s.id),
            escape_html(&subagent_summary(s))
        );
        out
    });
    format!("<h2>Subagents</h2><ul>{items}</ul>")
}

fn event_html(e: &ExportEvent) -> String {
    let who = escape_html(&one_line(who_of(e), LABEL_MAX));
    let tail = tail_of(e);
    let head = if tail.is_empty() {
        who
    } else {
        format!("{who} ({})", escape_html(&tail))
    };
    let note = e.note.as_deref().map_or_else(String::new, |n| {
        format!(
            "<p class=\"note\">{}</p>",
            escape_html(&one_line(n, NOTE_MAX))
        )
    });
    let body = format!("<pre>{}</pre>", escape_html(&e.body));
    if e.role != Role::Other {
        return format!(
            "<section class=\"msg {}\"><h3 class=\"who\">{head}</h3>{note}{body}</section>",
            e.role.as_str()
        );
    }
    let title = if !e.title.is_empty() && e.title != e.label {
        format!(" — {}", escape_html(&one_line_default(&e.title)))
    } else {
        String::new()
    };
    let class = if e.is_error == Some(true) {
        " class=\"err\""
    } else {
        ""
    };
    format!("<details{class}><summary>{head}{title}</summary>{note}{body}</details>")
}

/// Render a session as HTML.
pub fn render_html(session: &ParsedSession, opts: &ExportOptions) -> String {
    let d = prepare_export(session, opts);
    let title = escape_html(&one_line(&d.title, TITLE_MAX));
    let events = d
        .events
        .iter()
        .map(event_html)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<!doctype html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">
<meta name=\"referrer\" content=\"no-referrer\">
<title>{title}</title>
<style>{CSS}</style>
</head>
<body>
<h1>{title}</h1>
<dl>{}</dl>
{}
<h2>Timeline</h2>
{events}
</body>
</html>
",
        meta(&d),
        subagents(&d)
    )
}
