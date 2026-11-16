//! Capabilities: the feature x agent support matrix, its legend, and a
//! dialog with one cell's note.

use peektrace_protocol::{AgentId, Capability, SupportLevel};
use rune::{Color, El, ScrollSpec, col, row, text};
use rune_kit::{Badge, BadgeVariant, Dialog};

use super::blocks::{failure, pending};
use crate::model::capabilities::{
    AGENT_COLUMNS, CAPABILITY_HEAD, LEGEND_HEAD, LEVELS, NOTE_HEAD, cell_note, group_by,
    level_gloss, level_label, support,
};
use crate::state::Ui;
use crate::theme::{Theme, hex_or_grey, ty};

/// The inspector's `max-w-6xl` content.
const CONTENT_W: f32 = 1152.0;
const FIRST_COL: f32 = 0.4;

/// Fill, text and border of a level's pill (Tailwind 500 fills, 400 text).
fn level_colors(l: SupportLevel, t: &Theme) -> (Color, Color, Color) {
    let tint = |base: &str, ink: &str| {
        let b = hex_or_grey(base);
        (b.alpha(0.15), hex_or_grey(ink), b.alpha(0.3))
    };
    match l {
        SupportLevel::Supported => tint("#10b981", "#34d399"),
        SupportLevel::Partial => tint("#f59e0b", "#fbbf24"),
        SupportLevel::Planned => tint("#0ea5e9", "#38bdf8"),
        SupportLevel::Unsupported => (t.muted, t.muted_fg, t.border),
    }
}

fn pill(l: SupportLevel, t: &Theme) -> El {
    let (fill, ink, border) = level_colors(l, t);
    row()
        .h(22.0)
        .px(8.0)
        .radius(11.0)
        .center_y()
        .border(1.0, border)
        .fill(fill)
        .child(text(level_label(l)).font(ty::XS_MEDIUM).color(ink))
}

fn legend(t: &Theme) -> El {
    row()
        .key("cap-legend")
        .w_full()
        .flex_wrap()
        .gap(16.0)
        .children(LEVELS.into_iter().map(|l| {
            row()
                .gap(6.0)
                .center_y()
                .child(pill(l, t))
                .child(text(level_gloss(l)).font(ty::XS).color(t.muted_fg))
        }))
}

fn cell(cap: &Capability, agent: AgentId, t: &Theme) -> El {
    let level = support(cap, agent).level;
    let id = cap.id.clone();
    t.kit
        .interactive(row().radius(4.0).child(pill(level, t)))
        .key(format!("cap-cell-{}-{}", cap.id, agent.as_str()))
        .role("button")
        .label(level_label(level))
        .focusable(true)
        .on_press(move |s: &mut Ui| s.open_cell(&id, agent))
}

fn head_cell(label: &str, frac: f32, t: &Theme) -> El {
    row()
        .w_frac(frac)
        .h(40.0)
        .px(8.0)
        .center_y()
        .child(text(label.to_owned()).font(ty::SM_MEDIUM).color(t.fg))
}

fn matrix(caps: &[Capability], t: &Theme) -> El {
    let rest = (1.0 - FIRST_COL) / 4.0;
    let line = [0.0, 0.0, 1.0, 0.0];
    let mut m = col()
        .key("capability-matrix")
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .child(
            row()
                .w_full()
                .border_sides(line, t.border)
                .child(head_cell(CAPABILITY_HEAD, FIRST_COL, t))
                .children(
                    AGENT_COLUMNS
                        .iter()
                        .map(|(_, label)| head_cell(label, rest, t)),
                ),
        );
    for (group, rows) in group_by(caps) {
        m = m.child(
            row()
                .w_full()
                .px(8.0)
                .py(6.0)
                .fill(t.muted.alpha(0.4))
                .border_sides(line, t.border)
                .child(
                    text(group.to_uppercase())
                        .font(ty::XS_MEDIUM)
                        .color(t.muted_fg),
                ),
        );
        m = m.children(rows.into_iter().map(|cap| {
            row()
                .w_full()
                .items_start()
                .border_sides(line, t.border)
                .child(
                    col()
                        .w_frac(FIRST_COL)
                        .padding(8.0)
                        .gap(2.0)
                        .child(text(cap.title.clone()).font(ty::SM_MEDIUM).color(t.fg))
                        .child(
                            text(cap.description.clone())
                                .font(ty::XS)
                                .color(t.muted_fg)
                                .wrap()
                                .w_full(),
                        ),
                )
                .children(
                    AGENT_COLUMNS
                        .iter()
                        .map(|(a, _)| row().w_frac(rest).padding(8.0).child(cell(cap, *a, t))),
                )
        }));
    }
    m
}

fn cell_dialog(s: &Ui, caps: &[Capability], t: &Theme, layer: &mut Vec<El>) {
    let Some((id, agent)) = &s.caps.selection else {
        return;
    };
    let Some(cap) = caps.iter().find(|c| &c.id == id) else {
        return;
    };
    let sup = support(cap, *agent);
    let label = AGENT_COLUMNS
        .iter()
        .find(|(a, _)| a == agent)
        .map_or("", |(_, l)| *l);
    let heading = |s: &str| text(s.to_owned()).font(ty::SM_MEDIUM).color(t.fg);
    let body = col()
        .w_full()
        .gap(16.0)
        .px(16.0)
        .pb(16.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    Badge::new(label)
                        .variant(BadgeVariant::Subtle)
                        .build(&t.kit),
                )
                .child(pill(sup.level, t)),
        )
        .child(
            col().w_full().gap(4.0).child(heading(NOTE_HEAD)).child(
                text(cell_note(sup).to_owned())
                    .key("cap-cell-note")
                    .font(ty::SM)
                    .color(t.muted_fg)
                    .wrap()
                    .w_full(),
            ),
        )
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(heading(LEGEND_HEAD))
                .child(legend(t)),
        );
    Dialog::new("cap-cell-sheet", cap.title.clone())
        .description(cap.description.clone())
        .open(true)
        .fit()
        .on_close(|s: &mut Ui| s.caps.selection = None)
        .child(body)
        .build(&t.kit, layer);
}

pub fn page(s: &Ui, t: &Theme, layer: &mut Vec<El>) -> El {
    let body = match (&s.caps.list, &s.caps.error) {
        (_, Some(e)) => failure(e, s.caps.retrying, Ui::retry_capabilities, t),
        (None, None) => pending(t),
        (Some(caps), None) => {
            cell_dialog(s, caps, t, layer);
            col()
                .w_full()
                .gap(16.0)
                .child(matrix(caps, t))
                .child(legend(t))
        }
    };
    col()
        .key("capabilities-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col().w_full().items_center().padding(24.0).child(
                col()
                    .key("capabilities")
                    .w_full()
                    .max_w(CONTENT_W)
                    .child(body),
            ),
        )
}
