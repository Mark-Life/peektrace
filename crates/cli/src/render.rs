//! Pure stdout rendering: `JSON.stringify(rows, null, 2)`, the tab-separated
//! table, the padded `--pretty` table and the `sessions ls` notes.

use std::fmt::Write as _;

use peektrace_core::SessionHeader;
use peektrace_core::js::num;
use peektrace_core::js::text::{self, Unit};

use crate::width::display_width;

/// Append `units` as a JSON string, escaped the way `JSON.stringify` does.
fn push_json_units(out: &mut String, units: impl Iterator<Item = Unit>) {
    out.push('"');
    for u in units {
        match u {
            Unit::Char('"') => out.push_str("\\\""),
            Unit::Char('\\') => out.push_str("\\\\"),
            Unit::Char('\u{08}') => out.push_str("\\b"),
            Unit::Char('\u{0C}') => out.push_str("\\f"),
            Unit::Char('\n') => out.push_str("\\n"),
            Unit::Char('\r') => out.push_str("\\r"),
            Unit::Char('\t') => out.push_str("\\t"),
            Unit::Char(c) if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            Unit::Char(c) => out.push(c),
            Unit::Lone(unit) => {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out.push('"');
}

/// Append a plain string as JSON.
pub fn push_json_string(out: &mut String, s: &str) {
    push_json_units(out, s.chars().map(Unit::Char));
}

/// Append transcript text (see [`text::units`]) as JSON; unpaired
/// surrogates print as `\\udXXX` escapes.
fn push_json_text(out: &mut String, s: &str) {
    push_json_units(out, text::units(s));
}

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
fn header_fields(h: &SessionHeader) -> Vec<(&'static str, Value<'_>)> {
    let optional = [
        ("cwd", h.cwd.as_deref().map(Value::Text)),
        ("gitBranch", h.git_branch.as_deref().map(Value::Text)),
        ("model", h.model.as_deref().map(Value::Text)),
        ("title", h.title.as_deref().map(Value::Text)),
        ("startedAt", h.started_at.as_deref().map(Value::Text)),
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
        ("project", Value::Str(h.project.as_str())),
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
        Value::Str(s) => push_json_string(out, s),
        Value::Text(s) => push_json_text(out, s),
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
                push_json_text(out, item);
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
            push_json_string(&mut out, key);
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
fn usd_cell(n: Option<f64>) -> String {
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
        h.project.as_str().to_owned(),
        printable(h.model.as_deref().unwrap_or("-")),
        h.message_count.to_string(),
        format!("{}{star}", usd_cell(h.cost_usd)),
        bytes(h.size_bytes),
        printable(&h.updated_at),
        printable(h.title.as_deref().unwrap_or("-")),
    ]
}

/// Tab-separated table with lowercase headers; `(none)` when empty.
fn compact_table(rows: &[Vec<String>]) -> String {
    let head = HEADERS.map(str::to_lowercase).join("\t");
    if rows.is_empty() {
        return format!("{head}\n(none)");
    }
    std::iter::once(head)
        .chain(rows.iter().map(|r| r.join("\t")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Columns sized to their widest cell, two spaces apart, with a rule line.
fn pretty_table(rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = HEADERS
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
    let head = line(&HEADERS);
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

/// The `sessions ls` table and the notes it adds.
pub fn session_table(rows: &[SessionHeader], pretty: bool) -> (String, Vec<String>) {
    let cells: Vec<_> = rows.iter().map(row_cells).collect();
    let table = if pretty {
        pretty_table(&cells)
    } else {
        compact_table(&cells)
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_json_stringify() {
        let mut out = String::new();
        push_json_string(&mut out, "a\"b\\c\n\u{1}\u{7f}\u{2028}é");
        assert_eq!(out, "\"a\\\"b\\\\c\\n\\u0001\u{7f}\u{2028}é\"");
    }

    #[test]
    fn stand_ins_print_as_escapes() {
        let mut out = String::new();
        push_json_text(&mut out, "a\u{10F800}");
        assert_eq!(out, "\"a\\ud800\"");
        assert_eq!(printable("a\u{10FC00}"), "a\u{FFFD}");
        out.clear();
        push_json_string(&mut out, "a\u{10F800}");
        assert_eq!(out, "\"a\u{10F800}\"");
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
    }
}
