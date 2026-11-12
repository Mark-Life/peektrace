//! Pure stdout rendering: `JSON.stringify(rows, null, 2)`, the tab-separated
//! table, the padded `--pretty` table and the `sessions ls` notes.

use std::fmt::Write as _;

use peektrace_core::js::json_out::{push_str, push_text};
use peektrace_core::js::num;
use peektrace_core::js::text;
use peektrace_core::{Agent, SessionHeader};

use crate::width::display_width;

/// One JSON value in a header object.
enum Value<'a> {
    Str(&'a str),
    /// Transcript text.
    Text(&'a str),
    Int(u64),
    Float(f64),
    List(&'a [String]),
}

/// The header's keys and values in `--json` key order, absent ones dropped.
/// Pi puts `title` after `startedAt`; a Codex project comes from the cwd,
/// so it is transcript text.
fn header_fields(h: &SessionHeader) -> Vec<(&'static str, Value<'_>)> {
    let title = ("title", h.title.as_deref().map(Value::Text));
    let started = ("startedAt", h.started_at.as_deref().map(Value::Text));
    let (first, second) = if h.agent == Agent::Pi {
        (started, title)
    } else {
        (title, started)
    };
    let project = if h.agent == Agent::Codex {
        Value::Text(h.project.as_str())
    } else {
        Value::Str(h.project.as_str())
    };
    let optional = [
        ("cwd", h.cwd.as_deref().map(Value::Text)),
        ("gitBranch", h.git_branch.as_deref().map(Value::Text)),
        ("model", h.model.as_deref().map(Value::Text)),
        first,
        second,
        ("malformedLines", h.malformed_lines.map(Value::Int)),
        ("costUsd", h.cost_usd.map(Value::Float)),
        (
            "unpricedModels",
            h.unpriced_models.as_deref().map(Value::List),
        ),
        ("source", h.source.as_ref().map(|s| Value::Text(&s.id))),
        (
            "sourceLabel",
            h.source.as_ref().map(|s| Value::Text(&s.label)),
        ),
    ];
    [
        ("id", Value::Str(h.id.as_str())),
        ("agent", Value::Str(h.agent.as_str())),
        ("path", Value::Str(&h.path)),
        ("project", project),
        ("messageCount", Value::Int(h.message_count)),
        ("sizeBytes", Value::Int(h.size_bytes)),
        ("updatedAt", Value::Text(&h.updated_at)),
    ]
    .into_iter()
    .chain(optional.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))))
    .collect()
}

fn push_value(out: &mut String, v: &Value<'_>, indent: &str) {
    match v {
        Value::Str(s) => push_str(out, s),
        Value::Text(s) => push_text(out, s),
        Value::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Value::Float(x) => out.push_str(&num::to_json(*x)),
        Value::List([]) => out.push_str("[]"),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.push_str(indent);
                out.push_str("  ");
                push_text(out, item);
            }
            out.push('\n');
            out.push_str(indent);
            out.push(']');
        }
    }
}

/// `JSON.stringify(rows, null, 2)`.
pub fn json_rows(rows: &[SessionHeader]) -> String {
    if rows.is_empty() {
        return "[]".to_owned();
    }
    let mut out = String::from("[");
    for (i, h) in rows.iter().enumerate() {
        out.push_str(if i == 0 { "\n  {" } else { ",\n  {" });
        for (j, (key, value)) in header_fields(h).iter().enumerate() {
            out.push_str(if j == 0 { "\n    " } else { ",\n    " });
            push_str(&mut out, key);
            out.push_str(": ");
            push_value(&mut out, value, "    ");
        }
        out.push_str("\n  }");
    }
    out.push_str("\n]");
    out
}

const HEADERS: [&str; 9] = [
    "ID", "AGENT", "PROJECT", "MODEL", "MSGS", "EST COST", "SIZE", "UPDATED", "TITLE",
];
const CENT: f64 = 0.005;
const KB: f64 = 1024.0;

/// `~$1.23`, `<$0.01` below half a cent, `-` when absent.
pub fn usd_cell(n: Option<f64>) -> String {
    match n {
        None => "-".to_owned(),
        Some(n) if n > 0.0 && n < CENT => "<$0.01".to_owned(),
        Some(n) => format!("~${}", num::to_fixed(n, 2)),
    }
}

/// Human-readable size, 1 KB = 1024 B.
#[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
fn bytes(n: u64) -> String {
    let x = n as f64;
    if x < KB {
        format!("{n} B")
    } else if x < KB * KB {
        format!("{} KB", num::to_fixed(x / KB, 1))
    } else {
        format!("{} MB", num::to_fixed(x / (KB * KB), 1))
    }
}

/// First segment of a UUID-like id.
fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

/// A transcript text cell as the terminal gets it: an unpaired surrogate
/// prints as U+FFFD.
fn printable(cell: &str) -> String {
    text::to_display(cell).into_owned()
}

fn row_cells(h: &SessionHeader) -> Vec<String> {
    let star = if h.unpriced_models.is_some() { "*" } else { "" };
    vec![
        short_id(h.id.as_str()).to_owned(),
        h.agent.as_str().to_owned(),
        if h.agent == Agent::Codex {
            printable(h.project.as_str())
        } else {
            h.project.as_str().to_owned()
        },
        printable(h.model.as_deref().unwrap_or("-")),
        h.message_count.to_string(),
        format!("{}{star}", usd_cell(h.cost_usd)),
        bytes(h.size_bytes),
        printable(&h.updated_at),
        printable(h.title.as_deref().unwrap_or("-")),
    ]
}

/// Tab-separated table with lowercase headers; `(none)` when empty.
fn compact_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let head = headers
        .iter()
        .map(|h| h.to_lowercase())
        .collect::<Vec<_>>()
        .join("\t");
    if rows.is_empty() {
        return format!("{head}\n(none)");
    }
    std::iter::once(head)
        .chain(rows.iter().map(|r| r.join("\t")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Columns sized to their widest cell, two spaces apart, with a rule line.
fn pretty_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(col, h)| {
            rows.iter()
                .map(|r| display_width(&r[col]))
                .fold(display_width(h), usize::max)
        })
        .collect();
    let line = |cells: &[&str]| {
        let padded = cells
            .iter()
            .zip(&widths)
            .map(|(c, &w)| format!("{c}{}", " ".repeat(w.saturating_sub(display_width(c)))))
            .collect::<Vec<_>>()
            .join("  ");
        text::trim_end(&padded).to_owned()
    };
    let head = line(headers);
    let rule = widths
        .iter()
        .map(|&w| "-".repeat(w))
        .collect::<Vec<_>>()
        .join("  ");
    if rows.is_empty() {
        return format!("{head}\n{rule}\n(none)");
    }
    std::iter::once(head)
        .chain(std::iter::once(rule))
        .chain(
            rows.iter()
                .map(|r| line(&r.iter().map(String::as_str).collect::<Vec<_>>())),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

/// A table: padded for people with `pretty`, else tab-separated.
pub fn table(headers: &[&str], rows: &[Vec<String>], pretty: bool) -> String {
    if pretty {
        pretty_table(headers, rows)
    } else {
        compact_table(headers, rows)
    }
}

/// The `sessions ls` table and the notes it adds.
pub fn session_table(rows: &[SessionHeader], pretty: bool) -> (String, Vec<String>) {
    let cells: Vec<_> = rows.iter().map(row_cells).collect();
    let table = table(&HEADERS, &cells, pretty);
    let notes = [
        rows.iter()
            .any(|h| h.cost_usd.is_some())
            .then(|| "EST COST covers the main transcript only, without subagent files".to_owned()),
        rows.iter()
            .any(|h| h.unpriced_models.is_some())
            .then(|| "* some models unpriced: the figure is too low".to_owned()),
    ];
    (table, notes.into_iter().flatten().collect())
}

/// Notes in print order: unreadable count, table notes, then the limit hint.
pub fn list_notes(unreadable: usize, table_notes: Vec<String>, hidden: usize) -> Vec<String> {
    let noun = if unreadable == 1 {
        "session"
    } else {
        "sessions"
    };
    std::iter::once((unreadable > 0).then(|| format!("{unreadable} {noun} could not be read")))
        .chain(table_notes.into_iter().map(Some))
        .chain(std::iter::once((hidden > 0).then(|| {
            format!("{hidden} more hidden, use --limit 0 to show all")
        })))
        .flatten()
        .collect()
}

/// Names as prose: `a`, `a and b`, `a, b and c`.
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Note for agents left out because this build cannot read them; `verb`
/// says what was not done to their sessions (`listed`, `searched`).
pub fn skipped_note(skipped: &[Agent], verb: &str) -> Option<String> {
    let names = skipped.iter().map(|a| a.as_str()).collect::<Vec<_>>();
    (!names.is_empty()).then(|| {
        format!(
            "{} sessions are not {verb}: not supported yet",
            and_list(&names)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_ins_print_as_replacement() {
        assert_eq!(printable("a\u{10FC00}"), "a\u{FFFD}");
    }

    #[test]
    fn sizes_and_costs() {
        assert_eq!(bytes(62), "62 B");
        assert_eq!(bytes(1280), "1.3 KB");
        assert_eq!(bytes(1_229_922), "1.2 MB");
        assert_eq!(usd_cell(Some(0.0234)), "~$0.02");
        assert_eq!(usd_cell(Some(0.001)), "<$0.01");
        assert_eq!(usd_cell(Some(0.0)), "~$0.00");
        assert_eq!(usd_cell(None), "-");
        assert_eq!(usd_cell(Some(f64::INFINITY)), "~$Infinity");
    }

    #[test]
    fn notes_order() {
        assert_eq!(
            list_notes(2, vec!["x".to_owned()], 3),
            [
                "2 sessions could not be read",
                "x",
                "3 more hidden, use --limit 0 to show all"
            ]
        );
        assert_eq!(list_notes(0, vec![], 0), Vec::<String>::new());
        assert_eq!(
            skipped_note(&[Agent::Codex, Agent::Pi, Agent::Opencode], "listed").as_deref(),
            Some("codex, pi and opencode sessions are not listed: not supported yet")
        );
        assert_eq!(
            skipped_note(&[Agent::Codex], "searched").as_deref(),
            Some("codex sessions are not searched: not supported yet")
        );
        assert_eq!(skipped_note(&[], "listed"), None);
    }
}
