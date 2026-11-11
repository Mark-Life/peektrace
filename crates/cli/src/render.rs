//! Pure stdout rendering: `JSON.stringify(rows, null, 2)`, the tab-separated
//! table, the padded `--pretty` table and the `sessions ls` notes.

use std::fmt::Write as _;

use peektrace_core::SessionHeader;
use peektrace_core::js::{num, text};
use unicode_width::UnicodeWidthStr;

/// Append `s` as a JSON string, escaped the way `JSON.stringify` does.
pub fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c if let Some(unit) = text::stand_in_surrogate(c) => {
                let _ = write!(out, "\\u{unit:04x}");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// One JSON value in a header object.
enum Value<'a> {
    Str(&'a str),
    Int(u64),
    Float(f64),
    List(&'a [String]),
}

/// The header's keys and values in `--json` key order, absent ones dropped.
fn header_fields(h: &SessionHeader) -> Vec<(&'static str, Value<'_>)> {
    let optional = [
        ("cwd", h.cwd.as_deref().map(Value::Str)),
        ("gitBranch", h.git_branch.as_deref().map(Value::Str)),
        ("model", h.model.as_deref().map(Value::Str)),
        ("title", h.title.as_deref().map(Value::Str)),
        ("startedAt", h.started_at.as_deref().map(Value::Str)),
        ("malformedLines", h.malformed_lines.map(Value::Int)),
        ("costUsd", h.cost_usd.map(Value::Float)),
        (
            "unpricedModels",
            h.unpriced_models.as_deref().map(Value::List),
        ),
        ("source", h.source.as_ref().map(|s| Value::Str(&s.id))),
        (
            "sourceLabel",
            h.source.as_ref().map(|s| Value::Str(&s.label)),
        ),
    ];
    [
        ("id", Value::Str(h.id.as_str())),
        ("agent", Value::Str(h.agent.as_str())),
        ("path", Value::Str(&h.path)),
        ("project", Value::Str(h.project.as_str())),
        ("messageCount", Value::Int(h.message_count)),
        ("sizeBytes", Value::Int(h.size_bytes)),
        ("updatedAt", Value::Str(&h.updated_at)),
    ]
    .into_iter()
    .chain(optional.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))))
    .collect()
}

fn push_value(out: &mut String, v: &Value<'_>, indent: &str) {
    match v {
        Value::Str(s) => push_json_string(out, s),
        Value::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Value::Float(x) => out.push_str(&num::to_js_string(*x)),
        Value::List([]) => out.push_str("[]"),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.push_str(indent);
                out.push_str("  ");
                push_json_string(out, item);
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

/// A cell as the terminal gets it: an unpaired surrogate prints as U+FFFD.
fn printable(cell: String) -> String {
    if cell.chars().any(|c| text::stand_in_surrogate(c).is_some()) {
        cell.chars()
            .map(|c| text::stand_in_surrogate(c).map_or(c, |_| '\u{FFFD}'))
            .collect()
    } else {
        cell
    }
}

/// Terminal columns `s` takes, as `Bun.stringWidth` counts them: ANSI CSI
/// sequences and control chars take none.
fn display_width(s: &str) -> usize {
    if !s.chars().any(char::is_control) {
        return s.width();
    }
    let mut visible = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while chars
                .next()
                .is_some_and(|f| !('\u{40}'..='\u{7e}').contains(&f))
            {}
        } else if !c.is_control() {
            visible.push(c);
        }
    }
    visible.width()
}

fn row_cells(h: &SessionHeader) -> Vec<String> {
    let star = if h.unpriced_models.is_some() { "*" } else { "" };
    let cells = vec![
        short_id(h.id.as_str()).to_owned(),
        h.agent.as_str().to_owned(),
        h.project.as_str().to_owned(),
        h.model.clone().unwrap_or_else(|| "-".to_owned()),
        h.message_count.to_string(),
        format!("{}{star}", usd_cell(h.cost_usd)),
        bytes(h.size_bytes),
        h.updated_at.clone(),
        h.title.clone().unwrap_or_else(|| "-".to_owned()),
    ];
    cells.into_iter().map(printable).collect()
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
    fn widths_like_bun() {
        assert_eq!(display_width("日本語 🎉 title \u{1} \"q\" \\ end"), 26);
        assert_eq!(display_width("\u{1b}[31mred\u{1b}[0m"), 3);
        assert_eq!(display_width("\u{7f}\u{85}"), 0);
        assert_eq!(display_width("👨\u{200d}👩\u{200d}👧"), 2);
    }

    #[test]
    fn stand_ins_print_as_escapes() {
        let mut out = String::new();
        push_json_string(&mut out, "a\u{10F800}");
        assert_eq!(out, "\"a\\ud800\"");
        assert_eq!(printable("a\u{10FC00}".to_owned()), "a\u{FFFD}");
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
