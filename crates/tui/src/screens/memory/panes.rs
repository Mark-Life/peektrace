//! The memory detail panes: a vault's budget, types and index diff, or one
//! entry's metadata over its scrollable body. Meta rows each get one row.

use peektrace_core::memory::types::{MAX_INDEX_BYTES, MAX_INDEX_LINES, MemoryEntry, MemoryVault};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::draw::{Seg, panel, put, seg, wrap_text};
use crate::fmt::{clip, fmt_bytes, fmt_started};
use crate::theme::{self, fg};
use crate::widgets::{field, stacked_bar};

const TEXT_W: usize = 48;
const ROW_W: usize = TEXT_W - 4;
const VALUE_W: usize = TEXT_W - 16;
const LIST_CAP: usize = 6;
const DESC_ROWS: usize = 3;
const BODY_MAX: usize = 6000;
const LABEL_W: usize = 16;

fn bytes(n: usize) -> String {
    fmt_bytes(u64::try_from(n).unwrap_or(u64::MAX))
}

#[expect(clippy::cast_precision_loss, reason = "index line counts")]
const fn lines_f(n: usize) -> f64 {
    n as f64
}

fn rows_into(buf: &mut Buffer, inner: Rect, rows: &[Vec<Seg>]) -> u16 {
    let mut y = inner.y;
    for row in rows {
        if y >= inner.bottom() {
            break;
        }
        put(buf, inner.x, y, inner.width, row);
        y += 1;
    }
    y
}

pub fn vault(buf: &mut Buffer, r: Rect, v: &MemoryVault) {
    let inner = panel(buf, r, "Vault", false, 1);
    let b = v.budget;
    let text = fg(theme::TEXT);
    let mut rows = vec![
        vec![seg(clip(&v.project, TEXT_W), text)],
        vec![seg(clip(&v.slug, TEXT_W), fg(theme::TEXT_DIM))],
        vec![],
        field(
            "Index budget",
            &format!("{}/{MAX_INDEX_LINES} lines", b.lines),
            fg(if b.over_budget {
                theme::BAD
            } else {
                theme::TEXT
            }),
            LABEL_W,
        ),
        stacked_bar(
            &[
                (
                    lines_f(b.lines),
                    if b.over_budget {
                        theme::BAD
                    } else {
                        theme::GOOD
                    },
                ),
                (
                    lines_f(MAX_INDEX_LINES.saturating_sub(b.lines)),
                    theme::BORDER,
                ),
            ],
            ROW_W,
        ),
        field(
            "Bytes",
            &format!("{} / {}", bytes(b.bytes), bytes(MAX_INDEX_BYTES)),
            text,
            LABEL_W,
        ),
        vec![],
        vec![seg("Types", fg(theme::TEXT_DIM))],
    ];
    if v.type_counts.is_empty() {
        rows.push(vec![seg("  none", fg(theme::TEXT_FAINT))]);
    } else {
        let mut line = Vec::new();
        for (i, (t, n)) in v.type_counts.iter().enumerate() {
            if i > 0 {
                line.push(seg("  ", Style::new()));
            }
            line.push(seg(format!("{t} {n}"), text));
        }
        rows.push(line);
    }
    rows.push(vec![]);
    rows.push(vec![seg("Index ↔ files", fg(theme::TEXT_DIM))]);
    let d = &v.diff;
    if d.orphans.is_empty() && d.dangling.is_empty() {
        rows.push(vec![seg(
            "  index and files are in sync",
            fg(theme::TEXT_FAINT),
        )]);
    } else {
        rows.push(vec![seg(
            format!("  orphans {}", d.orphans.len()),
            fg(theme::WARN),
        )]);
        for f in d.orphans.iter().take(LIST_CAP) {
            rows.push(vec![seg(
                format!("    {}", clip(f, ROW_W)),
                fg(theme::TEXT_FAINT),
            )]);
        }
        rows.push(vec![seg(
            format!("  dangling {}", d.dangling.len()),
            fg(theme::BAD),
        )]);
        for x in d.dangling.iter().take(LIST_CAP) {
            rows.push(vec![seg(
                format!("    {} :{}", clip(&x.target, ROW_W - 4), x.line),
                fg(theme::TEXT_FAINT),
            )]);
        }
    }
    rows_into(buf, inner, &rows);
}

/// Draw an entry; returns the body scroll clamped to its rows.
pub fn entry(buf: &mut Buffer, r: Rect, e: &MemoryEntry, focused: bool, scroll: usize) -> usize {
    let inner = panel(buf, r, "Entry", focused, 1);
    let w = usize::from(inner.width);
    let mut rows = vec![vec![seg(
        clip(e.name().unwrap_or(&e.slug), TEXT_W),
        fg(theme::TEXT),
    )]];
    let index = if e.in_index {
        seg(" in index ✓", fg(theme::GOOD))
    } else {
        seg(" not in index ✗", fg(theme::BAD))
    };
    rows.push(vec![
        match e.kind() {
            None => seg("untyped", fg(theme::TEXT_FAINT)),
            Some(t) => seg(format!("[{t}]"), fg(theme::ACCENT)),
        },
        index,
    ]);
    if let Some(d) = e.description() {
        rows.extend(
            wrap_text(&clip(d, TEXT_W * DESC_ROWS), w, fg(theme::TEXT_DIM))
                .into_iter()
                .take(DESC_ROWS),
        );
    }
    rows.push(vec![]);
    let text = fg(theme::TEXT);
    rows.push(field(
        "Size",
        &format!("{} · {} lines", bytes(e.size), e.lines),
        text,
        LABEL_W,
    ));
    rows.push(field(
        "Modified",
        &fmt_started(Some(&e.mtime)),
        text,
        LABEL_W,
    ));
    rows.push(field(
        "Frontmatter",
        e.frontmatter.shape.as_str(),
        text,
        LABEL_W,
    ));
    rows.push(field("File", &clip(&e.file_name, VALUE_W), text, LABEL_W));
    if !e.links.is_empty() {
        rows.push(vec![seg(
            format!("Links ({})", e.links.len()),
            fg(theme::TEXT_DIM),
        )]);
        for l in e.links.iter().take(LIST_CAP) {
            rows.push(vec![seg(
                format!("  ↗ {}", clip(&l.raw_target, ROW_W)),
                fg(theme::INFO),
            )]);
        }
        if e.links.len() > LIST_CAP {
            rows.push(vec![seg(
                format!("  +{} more", e.links.len() - LIST_CAP),
                fg(theme::TEXT_FAINT),
            )]);
        }
    }
    rows.push(vec![seg("Body", fg(theme::TEXT_DIM))]);
    let y = rows_into(buf, inner, &rows);
    if y >= inner.bottom() {
        return 0;
    }
    let body = wrap_text(&crate::fmt::sanitize(&clip(&e.body, BODY_MAX)), w, text);
    let view = usize::from(inner.bottom() - y);
    let scroll = scroll.min(body.len().saturating_sub(view));
    for (i, row) in body.iter().skip(scroll).take(view).enumerate() {
        put(
            buf,
            inner.x,
            y + u16::try_from(i).unwrap_or(0),
            inner.width,
            row,
        );
    }
    scroll
}
