//! Pure stdout rendering: `JSON.stringify(rows, null, 2)`, the tab-separated
//! table, the padded `--pretty` table and the `sessions ls` notes.

use peektrace_core::js::num;
use peektrace_core::js::text;
use peektrace_core::sessions::write::headers_json;
use peektrace_core::{Agent, SessionHeader};

use crate::width::display_width;

/// `JSON.stringify(rows, null, 2)`.
pub fn json_rows(rows: &[SessionHeader]) -> String {
    headers_json(rows, true)
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
pub fn bytes(n: u64) -> String {
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
pub fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

/// A transcript text cell as the terminal gets it: an unpaired surrogate
/// prints as U+FFFD.
pub fn printable(cell: &str) -> String {
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

/// Narrowest the last column shrinks to when fitting a terminal.
const MIN_LAST_COLUMN: usize = 20;

/// Cut `value` to at most `max` columns, ending in `…` when clipped.
pub fn clip_to_width(value: &str, max: usize) -> String {
    if display_width(value) <= max {
        return value.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    let mut buf = [0u8; 4];
    for c in value.chars() {
        let w = display_width(c.encode_utf8(&mut buf));
        if used + w > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Columns sized to their widest cell, two spaces apart, with a rule line.
/// With `max_width` (the terminal width) the last column shrinks to fit,
/// down to `MIN_LAST_COLUMN`, and its cells are clipped.
fn pretty_table(headers: &[&str], rows: &[Vec<String>], max_width: Option<usize>) -> String {
    let mut widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(col, h)| {
            rows.iter()
                .map(|r| display_width(&r[col]))
                .fold(display_width(h), usize::max)
        })
        .collect();
    let last = widths.len().saturating_sub(1);
    let room = max_width.map(|m| {
        let fixed: usize = widths[..last].iter().map(|w| w + 2).sum();
        MIN_LAST_COLUMN.max(m.saturating_sub(fixed))
    });
    if let Some(room) = room
        && widths[last] > room
    {
        widths[last] = room;
    }
    let line = |cells: &[&str]| {
        let padded = cells
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(col, (c, &w))| {
                let c = match room {
                    Some(room) if col == last => clip_to_width(c, room),
                    _ => (*c).to_owned(),
                };
                let pad = w.saturating_sub(display_width(&c));
                format!("{c}{}", " ".repeat(pad))
            })
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
    fit_table(headers, rows, pretty, None)
}

/// [`table`] fitted to a terminal `max_width` columns wide (pretty only).
pub fn fit_table(
    headers: &[&str],
    rows: &[Vec<String>],
    pretty: bool,
    max_width: Option<usize>,
) -> String {
    if pretty {
        pretty_table(headers, rows, max_width)
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
    fn fits_the_last_column_to_the_terminal() {
        let rows = vec![vec!["a".to_owned(), "x".repeat(40)]];
        let t = fit_table(&["K", "V"], &rows, true, Some(30));
        assert_eq!(
            t,
            format!("K  V\n-  {}\na  {}…", "-".repeat(27), "x".repeat(26))
        );
        let narrow = fit_table(&["K", "V"], &rows, true, Some(5));
        assert!(
            narrow.ends_with(&format!("{}…", "x".repeat(19))),
            "{narrow}"
        );
        assert_eq!(
            fit_table(&["K", "V"], &rows, false, Some(5)),
            table(&["K", "V"], &rows, false)
        );
        assert_eq!(clip_to_width("ab", 2), "ab");
        assert_eq!(clip_to_width("日本語", 4), "日…");
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
