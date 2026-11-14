//! The Memory section, as the inspector's explorer: filters, the write
//! gate's line or banner, project overview cards and one collapsible
//! section per vault, plus the dialogs and toasts in the overlay layer.

mod dialogs;
mod graph;
mod paint;
mod table;
mod vault;

use peektrace_protocol::view::memory::AllVaultsView;
use rune::{Cx, El, ScrollSpec, Tx, col, el, icon, row, spacer, text};
use rune_kit::{Button, ControlSize, TextInput, glyphs};

use super::widgets::{grid, notice, select, skeleton, status};
use crate::model::memory::{
    self as m, ALL, Blocked, capability_title, crud_cells, empty_description, overview_line,
    support_summary, type_filter_options, visible_vaults,
};
use crate::source::LoadError;
use crate::state::Ui;
use crate::state_memory::{CreateTarget, Dialog};
use crate::theme::{Theme, ty};

const MAX_W: f32 = 1800.0;
const PAD: f32 = 16.0;
const SCROLL_KEY: &str = "mem-scroll";
const COL_KEY: &str = "mem-col";

/// The section's width: `outer` picks the breakpoints, as the inspector's
/// media queries do; `content` is what a full-width child gets.
#[derive(Debug, Clone, Copy)]
pub struct Width {
    pub outer: f32,
    pub content: f32,
}

impl Width {
    /// From the padded column's laid-out width.
    pub fn of(col: f32) -> Self {
        Self {
            outer: col,
            content: (col - 2.0 * PAD).min(MAX_W),
        }
    }
}

/// The key of a vault's collapsible trigger.
pub fn vault_key(slug: &str) -> String {
    format!("mem-vault:{slug}")
}

/// The key of a table row.
pub fn row_key(vault: &str, entry: &str) -> String {
    format!("mem-row:{vault}:{entry}")
}

fn error_text(e: &LoadError) -> String {
    match e {
        LoadError::Api(a) => m::wire_error_message(a),
        other => other.to_string(),
    }
}

fn retry(t: &Theme) -> El {
    Button::new("Retry")
        .control_size(ControlSize::Large)
        .on_press(|s: &mut Ui| s.request_memory())
        .build(&t.kit)
        .key("mem-retry")
}

fn create_anywhere(t: &Theme) -> El {
    Button::new("New memory in…")
        .control_size(ControlSize::Large)
        .on_press(|s: &mut Ui| s.memory.dialog = Some(Dialog::Create(CreateTarget::Pick)))
        .build(&t.kit)
        .key("mem-create-anywhere")
}

fn filters(s: &Ui, all: &AllVaultsView, can_write: bool, t: &Theme, layer: &mut Vec<El>) -> El {
    let mem = &s.memory;
    let search = TextInput::new(&mem.query)
        .placeholder("Search all memories…")
        .label("Search memories")
        .leading_icon(&glyphs::SEARCH)
        .on_edit(|s: &mut Ui| &mut s.memory.query)
        .build(&t.kit)
        .key("mem-search")
        .w(320.0);
    let types = type_filter_options();
    let type_labels: Vec<&str> = types.iter().map(|(_, l)| *l).collect();
    let current = types.iter().find(|(v, _)| *v == mem.ty).map(|(_, l)| *l);
    let ty = select("mem-type", &type_labels)
        .width(144.0)
        .current(current)
        .label("Type")
        .open(mem.type_open)
        .on_toggle(|s: &mut Ui| s.memory.type_open = !s.memory.type_open)
        .on_close(|s: &mut Ui| s.memory.type_open = false)
        .on_choose(|s: &mut Ui, picked: &str, _tx: &mut Tx| {
            if let Some((v, _)) = type_filter_options()
                .into_iter()
                .find(|(_, l)| *l == picked)
            {
                v.clone_into(&mut s.memory.ty);
            }
            s.memory.type_open = false;
        })
        .build(&t.kit, layer);
    let scopes: Vec<(String, String)> =
        std::iter::once((ALL.to_owned(), "All projects".to_owned()))
            .chain(
                all.vaults
                    .iter()
                    .map(|v| (v.slug.clone(), v.project.clone())),
            )
            .collect();
    let scope_labels: Vec<&str> = scopes.iter().map(|(_, l)| l.as_str()).collect();
    let current = scopes
        .iter()
        .find(|(v, _)| *v == mem.scope)
        .map(|(_, l)| l.as_str());
    let scope = select("mem-scope", &scope_labels)
        .width(208.0)
        .current(current)
        .label("Project")
        .open(mem.scope_open)
        .on_toggle(|s: &mut Ui| s.memory.scope_open = !s.memory.scope_open)
        .on_close(|s: &mut Ui| s.memory.scope_open = false)
        .on_choose(move |s: &mut Ui, picked: &str, _tx: &mut Tx| {
            if let Some((v, _)) = scopes.iter().find(|(_, l)| l == picked) {
                s.memory.scope.clone_from(v);
            }
            s.memory.scope_open = false;
        })
        .build(&t.kit, layer);
    let mut r = row()
        .key("mem-filters")
        .w_full()
        .gap(12.0)
        .center_y()
        .flex_wrap()
        .child(search)
        .child(ty)
        .child(scope)
        .child(spacer());
    if can_write {
        r = r.child(create_anywhere(t));
    }
    r
}

/// The summary line, or the banner when writes are blocked.
fn gate(s: &Ui, t: &Theme) -> Option<El> {
    let d = s.memory.data.as_ref()?;
    let v = s.memory.verdict()?;
    let cells = crud_cells(&d.caps);
    let Some(blocked) = v.blocked_by else {
        return Some(
            text(support_summary(&cells))
                .font(ty::XS)
                .color(t.muted_fg)
                .key("mem-support"),
        );
    };
    let (title, body) = match blocked {
        Blocked::ReadOnly => (m::READ_ONLY_TITLE.to_owned(), m::READ_ONLY_BODY.to_owned()),
        Blocked::Capability => (
            capability_title(v.level),
            v.note.clone().unwrap_or_else(|| support_summary(&cells)),
        ),
    };
    Some(
        row()
            .key("mem-banner")
            .w_full()
            .gap(10.0)
            .items_start()
            .radius(8.0)
            .border(1.0, t.border)
            .fill(t.card)
            .padding(12.0)
            .child(icon(&glyphs::LOCK, 16.0).color(t.fg))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(4.0)
                    .child(text(title).font(ty::SM_MEDIUM).color(t.fg))
                    .child(text(body).font(ty::SM).color(t.muted_fg).wrap()),
            ),
    )
}

fn empty_card(can_write: bool, t: &Theme) -> El {
    let mut c = col()
        .key("mem-empty")
        .w_full()
        .radius(12.0)
        .border(1.0, t.border)
        .fill(t.card)
        .padding(20.0)
        .gap(8.0)
        .child(text("No memories yet").font(ty::BASE_SEMI).color(t.fg))
        .child(
            text(empty_description(can_write))
                .font(ty::SM)
                .color(t.muted_fg)
                .wrap(),
        );
    if can_write {
        c = c.child(row().pt(8.0).child(create_anywhere(t)));
    }
    c
}

const fn overview_columns(width: Width) -> usize {
    if width.outer >= 1024.0 {
        4
    } else if width.outer >= 640.0 {
        2
    } else {
        1
    }
}

fn overview(all: &AllVaultsView, width: Width, t: &Theme) -> El {
    let cols = overview_columns(width);
    let gap = 12.0;
    #[expect(clippy::cast_precision_loss, reason = "a few columns")]
    let card_w = ((width.content - gap * (cols as f32 - 1.0)) / cols as f32).max(120.0);
    let cards = all.projects.iter().map(|p| {
        col()
            .key(format!("mem-card:{}", p.slug))
            .w(card_w)
            .radius(12.0)
            .border(1.0, t.border)
            .fill(t.card)
            .padding(16.0)
            .gap(4.0)
            .child(
                text(p.project.clone())
                    .font(ty::SM_MEDIUM)
                    .color(t.fg)
                    .truncate(),
            )
            .child(
                text(p.slug.clone())
                    .font(ty::MONO_XS)
                    .color(t.muted_fg)
                    .truncate(),
            )
            .child(
                text(overview_line(p))
                    .font(ty::XS)
                    .color(t.muted_fg)
                    .pt(6.0),
            )
    });
    row()
        .key("mem-overview")
        .w_full()
        .gap(gap)
        .flex_wrap()
        .children(cards)
}

fn explorer(s: &Ui, all: &AllVaultsView, width: Width, t: &Theme, layer: &mut Vec<El>) -> El {
    let mem = &s.memory;
    let can_write = mem.can_write();
    let mut c = col().w_full().max_w(MAX_W).gap(20.0).pb(48.0);
    if let Some(e) = &mem.error {
        c = c.child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(
                    text(error_text(e))
                        .font(ty::XS)
                        .color(t.destructive)
                        .key("mem-error")
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(retry(t)),
        );
    }
    let empty = all.projects.is_empty();
    if !empty {
        c = c.child(filters(s, all, can_write, t, layer));
    }
    c = c.children(gate(s, t));
    if empty {
        return c.child(empty_card(can_write, t));
    }
    c = c.child(overview(all, width, t));
    let shown = visible_vaults(all, &mem.scope, &mem.query.text, &mem.ty);
    if shown.is_empty() {
        return c.child(
            text(m::NO_MATCHES)
                .font(ty::SM)
                .color(t.muted_fg)
                .key("mem-no-matches"),
        );
    }
    c.child(
        col().key("mem-vaults").w_full().gap(0.0).children(
            shown
                .iter()
                .map(|v| vault::section(s, v, can_write, width, t)),
        ),
    )
}

pub fn view(s: &Ui, cx: &mut Cx<Ui>, t: &Theme, layer: &mut Vec<El>) -> El {
    let width = Width::of(cx.layout_size(COL_KEY).map_or(1100.0, |[w, _]| w));
    let body = match (&s.memory.data, &s.memory.error) {
        (Some(d), _) => explorer(s, &d.all, width, t, layer),
        (None, Some(e)) => {
            notice("Could not load memory", &error_text(e), t).child(row().pt(8.0).child(retry(t)))
        }
        (None, None) => pending(width, t),
    };
    dialogs::layer(s, t, layer);
    col()
        .key(SCROLL_KEY)
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col()
                .key(COL_KEY)
                .w_full()
                .padding(PAD)
                .items_center()
                .child(body),
        )
}

/// The skeleton while the first load runs: filters, overview cards and
/// two vault sections.
fn pending(width: Width, t: &Theme) -> El {
    let cols = overview_columns(width);
    let section = || {
        col()
            .w_full()
            .gap(12.0)
            .child(skeleton(Some(240.0), 20.0, t))
            .child(skeleton(None, 128.0, t))
    };
    col()
        .key("mem-loading")
        .w_full()
        .max_w(MAX_W)
        .gap(20.0)
        .child(status("Loading memory…", t))
        .child(
            row()
                .w_full()
                .gap(12.0)
                .flex_wrap()
                .child(skeleton(Some(320.0), 32.0, t))
                .child(skeleton(Some(144.0), 28.0, t))
                .child(skeleton(Some(208.0), 28.0, t)),
        )
        .child(grid(cols, 12.0, (0..cols).map(|_| skeleton(None, 84.0, t))))
        .child(section())
        .child(section())
}

/// A centred placeholder the size of a small chart.
fn hint(key: String, s: &str, t: &Theme) -> El {
    el().key(key)
        .child(text(s.to_owned()).font(ty::SM).color(t.muted_fg))
}
