//! One vault: its collapsible trigger, the budget, type and index diff
//! cards, the table heading, the browse table and the link graph.

use rune::{El, canvas, col, el, icon, row, text};
use rune_kit::{Button, ControlSize, glyphs};

use super::paint::{MONO_XS_SEMI, amber_400, chart_color, red_400, tone_color};
use super::{Width, graph, hint, table, vault_key};
use crate::chart::memory::{DONUT, budget_bars, donut, donut_radii, slice_polygon};
use crate::model::memory::{
    Shown, below_fold_text, capitalize, dangling_heading, orphan_heading, table_heading,
};
use crate::state::Ui;
use crate::state_memory::{CreateTarget, Dialog};
use crate::theme::{Theme, ty};
use crate::views::widgets::{badge, dot};

/// Wide enough for the inspector's three-column rows (`lg`).
const WIDE: f32 = 1024.0;

fn panel(key: String, title: &str, body: El, t: &Theme) -> El {
    col()
        .key(key)
        .w_full()
        .radius(12.0)
        .border(1.0, t.border)
        .fill(t.card)
        .padding(16.0)
        .gap(10.0)
        .child(text(title.to_owned()).font(ty::SM_MEDIUM).color(t.fg))
        .child(body)
}

#[expect(clippy::cast_possible_truncation, reason = "a percentage of a bar")]
fn gauge(v: &peektrace_protocol::view::memory::VaultView, t: &Theme) -> El {
    let key = format!("mem-budget:{}", v.slug);
    let bars = budget_bars(&v.budget);
    if bars.is_empty() {
        return text("No MEMORY.md index in this vault.")
            .font(ty::SM)
            .color(t.muted_fg)
            .key(key);
    }
    let mut c = col().key(key).w_full().gap(12.0);
    for b in &bars {
        let value = text(b.text.clone())
            .font(if b.over { MONO_XS_SEMI } else { ty::MONO_XS })
            .color(if b.over { red_400() } else { t.fg });
        c = c.child(
            col()
                .w_full()
                .gap(4.0)
                .child(
                    row()
                        .w_full()
                        .justify_between()
                        .child(text(b.label).font(ty::XS).color(t.muted_fg))
                        .child(value),
                )
                .child(
                    row().w_full().h(8.0).radius(4.0).fill(t.muted).child(
                        el().w_frac((b.pct / 100.0) as f32)
                            .h_full()
                            .radius(4.0)
                            .fill(tone_color(b.tone)),
                    ),
                ),
        );
    }
    if v.budget.below_fold_count > 0 {
        c = c.child(
            row()
                .key(format!("mem-below-fold:{}", v.slug))
                .w_full()
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .border(1.0, t.border)
                .fill(t.muted.alpha(0.5))
                .px(12.0)
                .py(8.0)
                .child(icon(&glyphs::EYE_OFF, 16.0).color(t.muted_fg))
                .child(
                    text(below_fold_text(v.budget.below_fold_count))
                        .font(ty::XS)
                        .color(t.muted_fg)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0),
                ),
        );
    }
    c
}

#[expect(clippy::cast_possible_truncation, reason = "viewBox units fit f32")]
fn donut_view(v: &peektrace_protocol::view::memory::VaultView, t: &Theme) -> El {
    let key = format!("mem-donut:{}", v.slug);
    let slices = donut(&v.type_counts);
    if slices.is_empty() {
        return hint(key, "No typed memories yet.", t);
    }
    let (outer, inner) = donut_radii(DONUT);
    let c = DONUT / 2.0;
    let shape = slices.iter().fold(
        rune::Shape::new().view_box(DONUT as f32, DONUT as f32),
        |sh, s| {
            let pts: Vec<[f32; 2]> = slice_polygon(s, c, c, outer, inner)
                .into_iter()
                .map(|[x, y]| [x as f32, y as f32])
                .collect();
            sh.fill(pts, chart_color(s.chart))
        },
    );
    let legend = col().gap(4.0).children(slices.iter().map(|s| {
        row()
            .gap(8.0)
            .center_y()
            .child(el().size(10.0, 10.0).radius(2.0).fill(chart_color(s.chart)))
            .child(text(capitalize(&s.kind)).font(ty::XS).color(t.fg))
            .child(
                text(s.count.to_string())
                    .font(ty::MONO_XS)
                    .color(t.muted_fg),
            )
    }));
    row()
        .key(key)
        .gap(16.0)
        .center_y()
        .child(
            canvas(shape)
                .size(DONUT as f32, DONUT as f32)
                .role("img")
                .label("Memory type distribution"),
        )
        .child(legend)
}

fn diff_view(v: &peektrace_protocol::view::memory::VaultView, t: &Theme) -> El {
    let key = format!("mem-diff:{}", v.slug);
    let d = &v.diff;
    if d.orphans.is_empty() && d.dangling.is_empty() {
        return hint(key, "Index and files are in sync.", t);
    }
    let item = |s: String| {
        row()
            .gap(8.0)
            .pl(12.0)
            .center_y()
            .child(dot(4.0, t.muted_fg))
            .child(text(s).font(ty::MONO_XS).color(t.muted_fg).wrap())
    };
    let mut c = col().key(key).w_full().gap(12.0);
    if !d.orphans.is_empty() {
        c = c.child(
            col()
                .gap(4.0)
                .child(
                    text(orphan_heading(d.orphans.len()))
                        .font(ty::SM_MEDIUM)
                        .color(amber_400()),
                )
                .children(d.orphans.iter().map(|o| item(o.clone()))),
        );
    }
    if !d.dangling.is_empty() {
        c = c.child(
            col()
                .gap(4.0)
                .child(
                    text(dangling_heading(d.dangling.len()))
                        .font(ty::SM_MEDIUM)
                        .color(red_400()),
                )
                .children(d.dangling.iter().map(|x| {
                    row()
                        .gap(4.0)
                        .pl(12.0)
                        .center_y()
                        .child(dot(4.0, t.muted_fg).mr(4.0))
                        .child(text(x.target.clone()).font(ty::MONO_XS).color(t.muted_fg))
                        .child(
                            text(format!("(line {})", x.line))
                                .font(ty::MONO_XS)
                                .color(t.muted_fg.alpha(0.6)),
                        )
                })),
        );
    }
    c
}

/// Cards side by side when wide, else stacked; `spans` are column shares.
fn grid(key: String, items: Vec<(El, f32)>, wide: bool) -> El {
    if wide {
        row().key(key).w_full().gap(16.0).items_start().children(
            items
                .into_iter()
                .map(|(e, share)| col().grow(share).w(0.0).min_w(0.0).child(e)),
        )
    } else {
        col()
            .key(key)
            .w_full()
            .gap(16.0)
            .children(items.into_iter().map(|(e, _)| e))
    }
}

fn heading(slug: &str, shown: usize, total: usize, can_write: bool, t: &Theme) -> El {
    let mut r = row().w_full().justify_between().center_y().pb(8.0).child(
        text(table_heading(shown, total))
            .font(ty::SM_MEDIUM)
            .color(t.fg)
            .key(format!("mem-heading:{slug}")),
    );
    if can_write {
        let target = CreateTarget::Fixed(slug.to_owned());
        r = r.child(
            Button::new("New memory")
                .control_size(ControlSize::Large)
                .on_press(move |s: &mut Ui| {
                    s.memory.dialog = Some(Dialog::Create(target.clone()));
                })
                .build(&t.kit)
                .key(format!("mem-create:{slug}")),
        );
    }
    r
}

fn trigger(s: &Ui, v: &Shown<'_>, open: bool, t: &Theme) -> El {
    let slug = v.vault.slug.clone();
    let mut label = row()
        .gap(8.0)
        .center_y()
        .grow(1.0)
        .min_w(0.0)
        .child(
            text(v.vault.project.clone())
                .font(ty::SM_MEDIUM)
                .color(t.fg)
                .truncate(),
        )
        .child(badge(&v.entries.len().to_string(), t.muted_fg, t));
    if v.vault.budget.over_budget {
        label = label.child(badge("over budget", t.destructive, t));
    }
    let focused = s.memory.focus.as_deref() == Some(slug.as_str());
    t.kit
        .interactive(
            row()
                .w_full()
                .h(52.0)
                .px(16.0)
                .gap(8.0)
                .center_y()
                .child(label)
                .child(
                    icon(
                        if open {
                            &glyphs::CHEVRON_UP
                        } else {
                            &glyphs::CHEVRON_DOWN
                        },
                        16.0,
                    )
                    .color(if focused { t.fg } else { t.muted_fg }),
                ),
        )
        .key(vault_key(&slug))
        .role("button")
        .label(v.vault.project.clone())
        .expanded(open)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.memory.toggle_vault(&slug))
}

/// A vault's section; its body only while open.
pub fn section(s: &Ui, v: &Shown<'_>, can_write: bool, width: Width, t: &Theme) -> El {
    let open = !s.memory.closed.contains(&v.vault.slug);
    let mut c = col()
        .key(format!("mem-section:{}", v.vault.slug))
        .w_full()
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .child(trigger(s, v, open, t));
    if !open {
        return c;
    }
    let wide = width.outer >= WIDE;
    let slug = &v.vault.slug;
    let cards = grid(
        format!("mem-cards:{slug}"),
        vec![
            (
                panel(
                    format!("mem-panel-budget:{slug}"),
                    "Index budget",
                    gauge(v.vault, t),
                    t,
                ),
                1.0,
            ),
            (
                panel(
                    format!("mem-panel-types:{slug}"),
                    "Types",
                    donut_view(v.vault, t),
                    t,
                ),
                1.0,
            ),
            (
                panel(
                    format!("mem-panel-diff:{slug}"),
                    "Index ↔ files",
                    diff_view(v.vault, t),
                    t,
                ),
                1.0,
            ),
        ],
        wide,
    );
    let inner = width.content - 32.0;
    let graph_w = if wide { (inner - 16.0) / 3.0 } else { inner } - 34.0;
    let table = col()
        .w_full()
        .child(heading(
            slug,
            v.entries.len(),
            v.vault.entries.len(),
            can_write,
            t,
        ))
        .child(table::table(s, v, can_write, t));
    let graph = panel(
        format!("mem-panel-graph:{slug}"),
        "Link graph",
        graph::graph(s, v.vault, graph_w, t),
        t,
    );
    let lower = grid(
        format!("mem-lower:{slug}"),
        vec![(table, 2.0), (graph, 1.0)],
        wide,
    );
    c = c.child(
        col()
            .key(format!("mem-body:{slug}"))
            .w_full()
            .px(16.0)
            .pb(16.0)
            .gap(16.0)
            .child(cards)
            .child(lower),
    );
    c
}
