//! A session as Markdown: chat messages stay raw Markdown; tool calls,
//! results, thinking and other events sit in fenced blocks.

use super::shared::{
    ExportEvent, ExportOptions, LABEL_MAX, NOTE_MAX, Role, TITLE_MAX, fence_for, meta_rows,
    one_line, one_line_default, prepare_export, subagent_summary, tail_of, who_of,
};
use crate::sessions::model::ParsedSession;

fn heading(e: &ExportEvent, position: usize) -> String {
    let tail = tail_of(e);
    let tail = if tail.is_empty() {
        String::new()
    } else {
        format!(" ({tail})")
    };
    format!("### {position}. {}{tail}", one_line(who_of(e), LABEL_MAX))
}

/// A fence line (up to 3 spaces, then 3+ backticks or tildes): its run and
/// info string. The info must hold no CR, U+2028 or U+2029, which the TS
/// `.` does not match.
fn fence_line(line: &str) -> Option<(&str, &str)> {
    let spaces = line.len() - line.trim_start_matches(' ').len();
    if spaces > 3 {
        return None;
    }
    let rest = &line[spaces..];
    let ch = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run_len = rest.len() - rest.trim_start_matches(ch).len();
    if run_len < 3 {
        return None;
    }
    let info = &rest[run_len..];
    (!info.contains(['\r', '\u{2028}', '\u{2029}'])).then(|| (&rest[..run_len], info))
}

/// Close a code fence the body leaves open, by appending its run.
fn close_open_fence(body: &str) -> Option<String> {
    let mut open: Option<&str> = None;
    for line in body.split('\n') {
        let Some((run, info)) = fence_line(line) else {
            continue;
        };
        match open {
            None => {
                // A backtick run with a backtick in its info string is an
                // inline span.
                if !(run.starts_with('`') && info.contains('`')) {
                    open = Some(run);
                }
            }
            Some(o) => {
                if run.starts_with(&o[..1])
                    && run.len() >= o.len()
                    && crate::js::text::trim(info).is_empty()
                {
                    open = None;
                }
            }
        }
    }
    open.map(|run| format!("{body}\n{run}"))
}

/// Close the last line-start `<!--` with no `-->` after it.
fn close_open_comment(body: &str) -> String {
    let lines: Vec<&str> = body.split('\n').collect();
    let opens = |l: &str| {
        let spaces = l.len() - l.trim_start_matches(' ').len();
        spaces <= 3 && l[spaces..].starts_with("<!--")
    };
    let Some(start) = lines.iter().rposition(|l| opens(l)) else {
        return body.to_owned();
    };
    let rest = lines[start..].join("\n");
    let after_open = rest.find("<!--").map_or("", |at| &rest[at + 4..]);
    if after_open.contains("-->") {
        body.to_owned()
    } else {
        format!("{body}\n-->")
    }
}

fn close_open_blocks(body: &str) -> String {
    close_open_fence(body).unwrap_or_else(|| close_open_comment(body))
}

fn event_block(e: &ExportEvent, i: usize) -> String {
    let mut parts = vec![heading(e, i + 1)];
    if e.role == Role::Other && !e.title.is_empty() && e.title != e.label {
        parts.push(format!("*{}*", one_line_default(&e.title)));
    }
    if let Some(note) = &e.note {
        parts.push(format!("> {}", one_line(note, NOTE_MAX)));
    }
    if e.role == Role::Other {
        let fence = fence_for(&e.body);
        parts.push(format!("{fence}\n{}\n{fence}", e.body));
    } else {
        parts.push(close_open_blocks(&e.body));
    }
    parts.join("\n\n")
}

/// Render a session as Markdown.
pub fn render_markdown(session: &ParsedSession, opts: &ExportOptions) -> String {
    let d = prepare_export(session, opts);
    let subs: Vec<String> = d
        .subagents
        .iter()
        .map(|s| format!("- `{}`: {}", s.id, subagent_summary(s)))
        .collect();
    let mut parts = vec![
        format!("# {}", one_line(&d.title, TITLE_MAX)),
        meta_rows(&d)
            .iter()
            .map(|(k, v)| format!("- {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n"),
    ];
    if !subs.is_empty() {
        parts.push("## Subagents".to_owned());
        parts.push(subs.join("\n"));
    }
    parts.push("## Timeline".to_owned());
    parts.extend(d.events.iter().enumerate().map(|(i, e)| event_block(e, i)));
    format!("{}\n", parts.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_blocks() {
        assert_eq!(close_open_blocks("a\n```js\nx"), "a\n```js\nx\n```");
        assert_eq!(close_open_blocks("```js\nx\n```"), "```js\nx\n```");
        assert_eq!(close_open_blocks("``` `a` ```"), "``` `a` ```");
        assert_eq!(close_open_blocks("```\r\nx"), "```\r\nx");
        assert_eq!(close_open_blocks("x\n<!-- a"), "x\n<!-- a\n-->");
        assert_eq!(close_open_blocks("<!-- a -->"), "<!-- a -->");
        assert_eq!(fence_for("a ```` b"), "`````");
        assert_eq!(fence_for("plain"), "```");
    }
}
