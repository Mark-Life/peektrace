//! Bodies: Markdown, highlighted code with big outputs cut, and diffs.

use rune::{El, Span, Tx, col, rich, row, text};
use rune_kit::syntax::{SyntaxTheme, highlight};
use rune_kit::{Button, ButtonVariant, ControlSize, Markdown};

use crate::model::diff::{LineDiff, RowKind};
use crate::model::format::fmt_bytes;
use crate::model::tools::EMPTY_BODY;
use crate::state::Ui;
use crate::theme::{Theme, ty};

/// Opened code longer than this shows its head and a button for the rest.
pub const CUT_BYTES: usize = 32 * 1024;
/// Past this the kit colours nothing, so the full text draws plain.
pub const PLAIN_BYTES: usize = 128 * 1024;
/// Markdown longer than this (in chars) renders as plain text.
pub const MARKDOWN_CHAR_LIMIT: usize = 200_000;

fn safe_url(url: &str) -> bool {
    ["http:", "https:", "mailto:"].iter().any(|p| {
        url.get(..p.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(p))
    })
}

/// `body` cut at the last line end within `CUT_BYTES`.
pub fn head_of(body: &str) -> &str {
    if body.len() <= CUT_BYTES {
        return body;
    }
    let mut end = CUT_BYTES;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    let head = &body[..end];
    head.rfind('\n').map_or(head, |i| &head[..i])
}

pub fn plain(s: &str, t: &Theme) -> El {
    text(if s.is_empty() {
        EMPTY_BODY.to_owned()
    } else {
        s.to_owned()
    })
    .font(ty::MONO_XS)
    .color(t.fg)
    .wrap_anywhere()
    .selectable(true)
}

pub fn markdown(src: &str, t: &Theme) -> El {
    if src.trim().is_empty() {
        return text(EMPTY_BODY).font(ty::SM.italic()).color(t.muted_fg);
    }
    if src.chars().count() > MARKDOWN_CHAR_LIMIT {
        return plain(src, t);
    }
    Markdown::new(src)
        .selectable(true)
        .images(|_| None)
        .on_link_tx(|_s: &mut Ui, url: &str, tx: &mut Tx| {
            if safe_url(url) {
                let _ = tx.open_url(url);
            }
        })
        .build(&t.kit)
}

fn highlighted(code: &str, language: &str, t: &Theme) -> El {
    let lang = (language != "text").then_some(language);
    let spans: Vec<Span> = match lang {
        Some(l) if code.len() <= PLAIN_BYTES => {
            SyntaxTheme::from_kit(&t.kit).spans(&highlight(Some(l), code), 0)
        }
        _ => Vec::new(),
    };
    rich(code.to_owned(), spans)
        .font(ty::MONO_XS)
        .color(t.fg)
        .wrap_anywhere()
        .selectable(true)
}

/// A code block. Over `CUT_BYTES` it shows the head and a button, unless
/// `full`; a full block over `PLAIN_BYTES` draws uncoloured.
pub fn code_block(key: &str, code: &str, language: &str, full: bool, t: &Theme) -> El {
    let code = if code.is_empty() { EMPTY_BODY } else { code };
    let frame = col()
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .fill(t.card)
        .padding(10.0)
        .gap(6.0);
    if code.len() > CUT_BYTES && !full {
        let id = key.to_owned();
        #[expect(clippy::cast_precision_loss, reason = "byte counts fit f64")]
        let size = fmt_bytes(code.len() as f64);
        return frame.child(highlighted(head_of(code), language, t)).child(
            Button::new(format!("Show full output ({size})"))
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut Ui| {
                    s.full.insert(id.clone());
                })
                .build(&t.kit)
                .key(format!("{key}:full")),
        );
    }
    let inner = if code.len() > PLAIN_BYTES {
        plain(code, t)
    } else {
        highlighted(code, language, t)
    };
    frame.child(inner)
}

pub fn diff_block(diff: &LineDiff, t: &Theme) -> El {
    let add = rune::color::hex("#3fb950").unwrap_or(t.fg);
    let del = rune::color::hex("#f85149").unwrap_or(t.fg);
    let rows = diff.rows.iter().map(|r| {
        let (sign, color, fill) = match r.kind {
            RowKind::Add => ("+", add, add.alpha(0.12)),
            RowKind::Del => ("-", del, del.alpha(0.12)),
            RowKind::Ctx => (" ", t.fg, rune::Color::TRANSPARENT),
            RowKind::Gap => ("", t.muted_fg, t.muted.alpha(0.4)),
        };
        let body = if r.kind == RowKind::Gap {
            format!(
                "⋯ {} unchanged line{}",
                r.count,
                if r.count == 1 { "" } else { "s" }
            )
        } else {
            format!("{sign} {}", r.text)
        };
        row()
            .w_full()
            .px(6.0)
            .fill(fill)
            .child(text(body).font(ty::MONO_XS).color(color).wrap_anywhere())
    });
    col()
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .fill(t.card)
        .py(6.0)
        .children(rows)
}
