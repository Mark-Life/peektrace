//! The calls behind one aggregate row, in a side sheet. Each hit opens its
//! session at the call; a page past the fetched one is a button.

use std::borrow::Cow;

use peektrace_js::num::to_js_string;
use peektrace_protocol::stats::{DrillHitView, DrillView};
use rune::{El, GrowthSpec, ScrollSpec, Tx, col, el, memo, row, spacer, text};
use rune_kit::{IconButton, IconButtonVariant};

use super::{V, error_view, mono, muted, outline_badge, small_button, title_for};
use crate::model::stats::chips::drill_label;
use crate::model::stats::format::hms;
use crate::model::stats::rows::page_note;
use crate::state::Ui;
use crate::theme::{Theme, theme, ty};
use crate::views::widgets::skeleton;

/// Sheet width, as `sm:max-w-xl`.
const SHEET_W: f32 = 576.0;
const HIT_ESTIMATE: f32 = 104.0;

fn hit_id(h: &DrillHitView) -> String {
    format!("{}:{}", h.sample.sid, to_js_string(h.sample.seq))
}

fn failed_badge(t: &Theme) -> El {
    row()
        .h(20.0)
        .px(8.0)
        .radius(6.0)
        .center_y()
        .fill(t.destructive.alpha(0.15))
        .child(text("failed").font(ty::XS_MEDIUM).color(t.destructive))
}

fn hit_view(s: &Ui, h: &DrillHitView, t: &Theme) -> El {
    let sm = &h.sample;
    let titles = title_for(s, sm);
    let mut line1 = row()
        .w_full()
        .gap(8.0)
        .items_end()
        .child(text(h.tool.clone()).font(ty::XS_MEDIUM).color(t.fg));
    if h.pair_line.is_some() {
        line1 = line1.child(outline_badge("retried", false, t));
    }
    if h.failed {
        line1 = line1.child(failed_badge(t));
    }
    line1 = line1.child(spacer()).child(
        text(h.dur_sec.map_or_else(|| "—".to_owned(), hms))
            .font(ty::MONO_XS)
            .color(t.muted_fg),
    );
    let meta = format!(
        "{} · {} · {}",
        drill_label(h, &titles),
        sm.kind,
        sm.ts.as_deref().unwrap_or("no timestamp")
    );
    let mut body = col()
        .w_full()
        .gap(4.0)
        .px(8.0)
        .py(6.0)
        .radius(6.0)
        .border(1.0, t.border)
        .child(line1)
        .child(mono(sm.line.clone(), t));
    if let Some(p) = &h.pair_line {
        body = body.child(text(format!("→ {p}")).font(ty::MONO_XS).color(t.fg).wrap());
    }
    body = body.child(
        text(meta)
            .font(ty::MONO_XS)
            .color(t.muted_fg.alpha(0.7))
            .wrap(),
    );
    let (sid, parent, call) = (sm.sid.clone(), sm.parent.clone(), sm.tool_use_id.clone());
    let link = t
        .kit
        .interactive(body)
        .key(format!("drill-hit:{}", hit_id(h)))
        .role("link")
        .label(format!("{} {}", h.tool, sm.line))
        .help(format!(
            "{} · {}",
            sm.project,
            sm.parent.as_deref().unwrap_or(&sm.sid)
        ))
        .focusable(true)
        .on_press_tx(move |s: &mut Ui, tx: &mut Tx| {
            s.open_sample(&sid, parent.clone(), call.clone(), tx);
        });
    let mut c = col().w_full().gap(4.0).pb(8.0).child(link);
    if h.pair_line.is_some() {
        let (sid, parent, call) = (
            sm.sid.clone(),
            sm.parent.clone(),
            h.pair_tool_use_id.clone(),
        );
        c = c.child(
            row().w_full().child(spacer()).child(
                small_button("Open the retry", true, t)
                    .on_press_tx(move |s: &mut Ui, tx: &mut Tx| {
                        s.open_sample(&sid, parent.clone(), call.clone(), tx);
                    })
                    .build(&t.kit)
                    .key(format!("drill-retry:{}", hit_id(h))),
            ),
        );
    }
    c
}

fn shown_page(s: &Ui) -> Option<&DrillView> {
    s.stats.drill_slot()?.value.as_ref()
}

fn hits(d: &DrillView, t: &Theme) -> El {
    let dark = t.dark;
    let version = (d.offset.to_bits(), d.hits.len(), d.key.clone());
    let mut c = col()
        .key("drill-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .px(16.0)
        .pb(24.0)
        .child(
            col()
                .key("drill-hits")
                .w_full()
                .growth(GrowthSpec::default())
                .virtual_rows(
                    d.hits.len(),
                    HIT_ESTIMATE,
                    |s: &Ui, i| {
                        shown_page(s)
                            .and_then(|d| d.hits.get(i))
                            .map_or(Cow::Borrowed("drill-hit:?"), |h| {
                                Cow::Owned(format!("drill-hit-row:{}", hit_id(h)))
                            })
                    },
                    move |s: &Ui, i| {
                        let Some(h) = shown_page(s).and_then(|d| d.hits.get(i)) else {
                            return el();
                        };
                        let t = theme(dark);
                        memo(
                            format!("drill-hit-row:{}", hit_id(h)),
                            (dark, s.rail.rev),
                            || hit_view(s, h, &t),
                        )
                    },
                )
                .virtual_rows_version(version),
        );
    let p = page_note(d.offset, d.hits.len(), d.total);
    let mut note = row()
        .w_full()
        .flex_wrap()
        .gap(8.0)
        .center_y()
        .pt(4.0)
        .child(muted(p.note, t).key("drill-note"));
    if let Some((label, next)) = p.more {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "offsets"
        )]
        let next = next as u32;
        note = note.child(
            small_button(label, false, t)
                .on_press(move |s: &mut Ui| s.drill_page(next))
                .build(&t.kit)
                .key("drill-more"),
        );
    }
    if let Some(back) = p.back {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "offsets"
        )]
        let back = back as u32;
        note = note.child(
            small_button("Back", true, t)
                .on_press(move |s: &mut Ui| s.drill_page(back))
                .build(&t.kit)
                .key("drill-back"),
        );
    }
    c = c.child(note);
    c
}

fn body(v: &V<'_>) -> El {
    let t = v.t;
    let Some(slot) = v.s.stats.drill_slot() else {
        return el();
    };
    if let Some(e) = &slot.error {
        return error_view(e, slot.loading, |s, _| s.drill_retry(), t);
    }
    slot.value.as_ref().map_or_else(
        || {
            col()
                .key("drill-pending")
                .w_full()
                .gap(12.0)
                .px(16.0)
                .child(skeleton(None, 48.0, t))
                .child(skeleton(None, 128.0, t))
                .child(skeleton(None, 128.0, t))
        },
        |d| hits(d, t),
    )
}

/// The drill sheet, pushed into `layer` while a drill is open.
pub fn sheet(v: &V<'_>, layer: &mut Vec<El>) {
    let t = v.t;
    let Some((_, key)) = &v.s.stats.drill else {
        return;
    };
    let close = IconButton::new(t.kit.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .label("Close")
        .on_press(|s: &mut Ui| s.close_drill())
        .build(&t.kit)
        .key("drill-close");
    let panel = col()
        .key("stats-drill")
        .role("dialog")
        .label(key.clone())
        .w_full()
        .max_w(SHEET_W)
        .h_full()
        .gap(12.0)
        .pt(16.0)
        .fill(t.bg)
        .border_sides([1.0, 0.0, 0.0, 0.0], t.border)
        .modal(true, |s: &mut Ui| s.close_drill())
        .child(
            col()
                .w_full()
                .gap(6.0)
                .px(16.0)
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(key.clone())
                                .font(ty::BASE_SEMI)
                                .color(t.fg)
                                .truncate()
                                .grow(1.0)
                                .min_w(0.0),
                        )
                        .child(close),
                )
                .child(muted(
                    "Every call behind this row, in the order it ran. Click one to open its session.",
                    t,
                )),
        )
        .child(body(v));
    layer.push(
        el().key("stats-drill-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(rune::Color::BLACK.alpha(0.4))
            .a11y_hidden(),
    );
    layer.push(
        row()
            .key("stats-drill-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .justify_end()
            .child(panel),
    );
}
