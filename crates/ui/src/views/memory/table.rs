//! The browse table: sortable headers, rows that expand in place, and the
//! detail row with the file name, write buttons, link chips and the body
//! with its `[[wikilinks]]` pressable.

use std::collections::HashMap;

use peektrace_js::time::LocalZone;
use peektrace_protocol::view::memory::EntryView;
use rune::{El, Span, Tx, col, icon, rich, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, glyphs};

use super::paint::{emerald_400, sky_400};
use super::row_key;
use crate::model::memory::{
    BodyPart, NO_MATCHES, Shown, SortKey, body_parts, capitalize, fmt_date, fmt_size, sort_entries,
    title_of,
};
use crate::state::Ui;
use crate::theme::{Theme, ty};
use crate::views::widgets::badge;

const ROW_H: f32 = 40.0;
/// Column widths; title and description share what is left.
const TYPE_W: f32 = 96.0;
const SIZE_W: f32 = 72.0;
const DATE_W: f32 = 176.0;
const INDEX_W: f32 = 72.0;
const LINKS_W: f32 = 56.0;
const DESC_MAX: f32 = 200.0;

/// The zone the Modified column is shown in.
fn zone() -> &'static dyn LocalZone {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static ZONE: std::sync::LazyLock<peektrace_core::js::system_zone::SystemZone> =
            std::sync::LazyLock::new(Default::default);
        &*ZONE
    }
    #[cfg(target_arch = "wasm32")]
    {
        &peektrace_js::time::Utc
    }
}

#[derive(Clone, Copy)]
enum Align {
    Start,
    End,
}

fn cell(e: El, w: Option<f32>, align: Align) -> El {
    let c = row().h_full().center_y().px(8.0);
    let c = match w {
        Some(w) => c.w(w),
        None => c.grow(1.0).w(0.0).min_w(0.0),
    };
    match align {
        Align::Start => c.child(e),
        Align::End => c.justify_end().child(e),
    }
}

fn head(vault: &str, label: &'static str, key: Option<SortKey>, s: &Ui, t: &Theme) -> El {
    let Some(key) = key else {
        return text(label).font(ty::XS_MEDIUM).color(t.muted_fg);
    };
    let sort = s.memory.sort_of(vault);
    let mut r = row()
        .gap(4.0)
        .center_y()
        .child(text(label).font(ty::XS_MEDIUM).color(t.muted_fg));
    if sort.key == key {
        let i = if sort.asc {
            &glyphs::CHEVRON_UP
        } else {
            &glyphs::CHEVRON_DOWN
        };
        r = r.child(icon(i, 12.0).color(t.muted_fg));
    }
    let vault = vault.to_owned();
    t.kit
        .interactive(r.radius(4.0).px(2.0))
        .key(format!("mem-sort:{vault}:{}", key.as_str()))
        .role("button")
        .label(format!("Sort by {label}"))
        .focusable(true)
        .on_press(move |s: &mut Ui| s.memory.press_sort(&vault, key))
}

fn header(vault: &str, s: &Ui, t: &Theme) -> El {
    let h = |label, key| head(vault, label, key, s, t);
    row()
        .w_full()
        .h(36.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .child(cell(h("Title", Some(SortKey::Title)), None, Align::Start))
        .child(cell(
            h("Type", Some(SortKey::Type)),
            Some(TYPE_W),
            Align::Start,
        ))
        .child(cell(h("Description", None), None, Align::Start))
        .child(cell(
            h("Size", Some(SortKey::Size)),
            Some(SIZE_W),
            Align::End,
        ))
        .child(cell(
            h("Modified", Some(SortKey::Mtime)),
            Some(DATE_W),
            Align::Start,
        ))
        .child(cell(
            h("In index", Some(SortKey::InIndex)),
            Some(INDEX_W),
            Align::Start,
        ))
        .child(cell(
            h("Links", Some(SortKey::Links)),
            Some(LINKS_W),
            Align::End,
        ))
}

fn entry_row(vault: &str, e: &EntryView, open: bool, t: &Theme) -> El {
    let kind = e.kind.as_deref().map_or_else(
        || text("—").font(ty::XS).color(t.muted_fg),
        |k| badge(&capitalize(k), t.muted_fg, t),
    );
    let in_index = if e.in_index {
        icon(&glyphs::CHECK, 16.0).color(emerald_400())
    } else {
        icon(&glyphs::CLOSE, 16.0).color(t.muted_fg)
    };
    let r = row()
        .w_full()
        .h(ROW_H)
        .fill(if open {
            t.muted.alpha(0.4)
        } else {
            rune::Color::TRANSPARENT
        })
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .child(cell(
            text(title_of(e).to_owned())
                .font(ty::SM_MEDIUM)
                .color(t.fg)
                .truncate(),
            None,
            Align::Start,
        ))
        .child(cell(kind, Some(TYPE_W), Align::Start))
        .child(cell(
            text(e.description.clone().unwrap_or_else(|| "—".to_owned()))
                .font(ty::SM)
                .color(t.muted_fg)
                .truncate()
                .max_w(DESC_MAX),
            None,
            Align::Start,
        ))
        .child(cell(
            text(fmt_size(e.size)).font(ty::MONO_XS).color(t.fg),
            Some(SIZE_W),
            Align::End,
        ))
        .child(cell(
            text(fmt_date(&e.mtime, zone()))
                .font(ty::XS)
                .color(t.muted_fg),
            Some(DATE_W),
            Align::Start,
        ))
        .child(cell(in_index, Some(INDEX_W), Align::Start))
        .child(cell(
            text(e.links.len().to_string())
                .font(ty::MONO_XS)
                .color(t.fg),
            Some(LINKS_W),
            Align::End,
        ));
    let (v, slug) = (vault.to_owned(), e.slug.clone());
    t.kit
        .interactive(r)
        .key(row_key(vault, &e.slug))
        .role("button")
        .label(title_of(e).to_owned())
        .expanded(open)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.memory.toggle_row(&v, &slug))
}

/// Open `slug` in `vault`, and bring its row into view when it has one.
pub(super) fn navigate(s: &mut Ui, vault: &str, slug: &str, tx: &mut Tx) {
    s.navigate_memory(vault, slug);
    let shown = s.memory.all().is_some_and(|a| {
        a.vaults
            .iter()
            .find(|v| v.slug == vault)
            .is_some_and(|v| v.entries.iter().any(|e| e.slug == slug))
    });
    if shown {
        tx.scroll_into_view(row_key(vault, slug));
    }
}

fn body(vault: &str, e: &EntryView, t: &Theme) -> El {
    let mut src = String::new();
    let mut spans = Vec::new();
    let mut links = Vec::new();
    let mut targets: HashMap<String, String> = HashMap::new();
    for (i, part) in body_parts(e).into_iter().enumerate() {
        match part {
            BodyPart::Text(s) => src.push_str(s),
            BodyPart::Link { text, slug } => {
                let at = src.len();
                src.push_str(text);
                let id = format!("mem-wiki:{vault}:{}:{i}", e.slug);
                spans.push(
                    Span::new(at, src.len())
                        .color(sky_400())
                        .underline(sky_400()),
                );
                links.push(rune::Link::new(at..src.len(), id.clone()).role("button"));
                targets.insert(id, slug);
            }
        }
    }
    let v = vault.to_owned();
    links
        .into_iter()
        .fold(rich(src, spans), El::link)
        .font(ty::MONO_XS)
        .color(t.fg)
        .wrap_anywhere()
        .selectable(true)
        .key(format!("mem-text:{vault}:{}", e.slug))
        .on_link(move |s: &mut Ui, id: &str, tx: &mut Tx| {
            if let Some(slug) = targets.get(id) {
                navigate(s, &v, slug, tx);
            }
        })
}

fn detail(vault: &str, e: &EntryView, can_write: bool, t: &Theme) -> El {
    let mut top = row().w_full().justify_between().center_y().gap(8.0).child(
        text(e.file_name.clone())
            .font(ty::MONO_XS)
            .color(t.muted_fg)
            .truncate(),
    );
    if can_write {
        let (v1, e1) = (vault.to_owned(), e.clone());
        let (v2, e2) = (vault.to_owned(), e.clone());
        top = top.child(
            row()
                .gap(8.0)
                .child(
                    Button::new("Edit")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut Ui| s.open_edit(&v1, &e1))
                        .build(&t.kit)
                        .key(format!("mem-edit:{vault}:{}", e.slug)),
                )
                .child(
                    Button::new("Delete")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut Ui| s.open_delete(&v2, &e2))
                        .build(&t.kit)
                        .key(format!("mem-delete:{vault}:{}", e.slug)),
                ),
        );
    }
    let mut c = col()
        .key(format!("mem-detail:{vault}:{}", e.slug))
        .w_full()
        .fill(t.muted.alpha(0.3))
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .px(12.0)
        .py(12.0)
        .gap(12.0)
        .child(top);
    if !e.links.is_empty() {
        let chips = e.links.iter().map(|l| {
            let (v, slug) = (vault.to_owned(), l.target_slug.clone());
            t.kit
                .interactive(
                    row()
                        .h(22.0)
                        .px(6.0)
                        .radius(4.0)
                        .border(1.0, t.border)
                        .center_y()
                        .child(
                            text(l.raw_target.clone())
                                .font(ty::MONO_XS)
                                .color(sky_400()),
                        ),
                )
                .key(format!(
                    "mem-link:{vault}:{}:{}:{}",
                    e.slug, l.target_slug, l.line
                ))
                .role("button")
                .label(l.raw_target.clone())
                .focusable(true)
                .on_press_tx(move |s: &mut Ui, tx: &mut Tx| navigate(s, &v, &slug, tx))
        });
        c = c.child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .flex_wrap()
                .child(text("Links:").font(ty::XS).color(t.muted_fg))
                .children(chips),
        );
    }
    c.child(body(vault, e, t))
}

pub fn table(s: &Ui, v: &Shown<'_>, can_write: bool, t: &Theme) -> El {
    let slug = &v.vault.slug;
    if v.entries.is_empty() {
        return text(NO_MATCHES)
            .font(ty::SM)
            .color(t.muted_fg)
            .key(format!("mem-table-empty:{slug}"));
    }
    let expanded = s.memory.expanded.get(slug);
    let rows = sort_entries(&v.entries, s.memory.sort_of(slug))
        .into_iter()
        .flat_map(|e| {
            let open = expanded.is_some_and(|x| *x == e.slug);
            let mut out = vec![entry_row(slug, e, open, t)];
            if open {
                out.push(detail(slug, e, can_write, t));
            }
            out
        });
    col()
        .key(format!("mem-table:{slug}"))
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .clip_rounded(8.0)
        .child(header(slug, s, t))
        .children(rows)
}
