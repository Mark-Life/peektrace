//! The route back: from a stats row to the sessions its samples came from.
//! Samples of one session fold into one chip; a subagent call names its
//! parent session and opens the subagent through it.

use peektrace_js::num::to_js_string;
use peektrace_protocol::stats::SampleRefView;
use rune::{El, col, icon, row, text};
use rune_kit::Popover;

use super::{ARROW_UP_RIGHT, V, small_button};
use crate::model::stats::chips::{chip_label_of, session_chips, target_label, title_of};
use crate::state::Ui;
use crate::theme::ty;

fn sample_id(s: &SampleRefView) -> String {
    format!("{}:{}", s.sid, to_js_string(s.seq))
}

fn open(s: &SampleRefView) -> impl Fn(&mut Ui) + 'static {
    let (sid, parent, call) = (s.sid.clone(), s.parent.clone(), s.tool_use_id.clone());
    move |ui: &mut Ui| ui.open_sample(&sid, parent.clone(), call.clone())
}

fn chip(v: &V<'_>, group: &[&SampleRefView]) -> El {
    let t = v.t;
    let first = group[0];
    let c = chip_label_of(first, v.titles);
    let subagent = group.iter().any(|s| s.parent.is_some());
    let mut body = row()
        .h(24.0)
        .px(8.0)
        .gap(4.0)
        .radius(6.0)
        .center_y()
        .min_w(0.0)
        .border(1.0, t.border)
        .child(icon(&ARROW_UP_RIGHT, 12.0).color(t.fg))
        .child(
            text(c.label.clone())
                .font(ty::MONO_XS)
                .color(t.fg)
                .truncate()
                .min_w(0.0),
        );
    if let Some(id) = &c.id {
        body = body.child(text(format!(" · {id}")).font(ty::MONO_XS).color(t.muted_fg));
    }
    if subagent {
        body = body.child(text(" · subagent").font(ty::MONO_XS).color(t.muted_fg));
    }
    t.kit
        .interactive(body)
        .key(format!("stats-session-link:{}", sample_id(first)))
        .role("link")
        .label(c.label)
        .help(title_of(group))
        .focusable(true)
        .on_press(open(first))
}

fn targets(v: &V<'_>, owner: &str, group: &[&SampleRefView], layer: &mut Vec<El>) -> El {
    let t = v.t;
    let first = group[0];
    let id = format!("{owner}/{}", sample_id(first));
    let open_now = v.s.stats.targets_open.as_deref() == Some(id.as_str());
    let toggle_id = id.clone();
    let trigger = small_button(format!("×{}", group.len()), true, t)
        .help(format!("Open one of {} samples", group.len()))
        .on_press(move |s: &mut Ui| {
            let open = s.stats.targets_open.as_deref() == Some(toggle_id.as_str());
            s.stats.targets_open = (!open).then(|| toggle_id.clone());
        })
        .build(&t.kit)
        .key(format!("stats-session-more:{}", sample_id(first)))
        .label(format!("Open one of {} samples", group.len()));
    let list = col()
        .key("stats-session-targets")
        .w(296.0)
        .gap(4.0)
        .children(group.iter().map(|s| {
            t.kit
                .interactive(
                    row()
                        .w_full()
                        .h(24.0)
                        .px(8.0)
                        .gap(4.0)
                        .radius(6.0)
                        .center_y()
                        .child(icon(&ARROW_UP_RIGHT, 12.0).color(t.fg))
                        .child(
                            text(target_label(s))
                                .font(ty::MONO_XS)
                                .color(t.fg)
                                .truncate()
                                .min_w(0.0),
                        ),
                )
                .key(format!("stats-session-target:{}", sample_id(s)))
                .role("link")
                .label(target_label(s))
                .help(s.line.clone())
                .focusable(true)
                .on_press(open(s))
        }));
    Popover::new(format!("stats-targets-{}", id.replace(['/', ':'], "-")))
        .trigger(trigger)
        .label("Samples")
        .open(open_now)
        .padding(8.0)
        .child(list)
        .on_close(|s: &mut Ui| s.stats.targets_open = None)
        .build(&t.kit, layer)
}

/// Sample links for one row, with the first sample's line under them.
/// `owner` names the row, so two rows' sample lists stay apart.
pub fn links(v: &V<'_>, owner: &str, samples: &[SampleRefView], layer: &mut Vec<El>) -> Option<El> {
    let first = samples.first()?;
    let chips = session_chips(samples).into_iter().map(|g| {
        let mut r = row().gap(2.0).center_y().min_w(0.0).child(chip(v, &g));
        if g.len() > 1 {
            r = r.child(targets(v, owner, &g, layer));
        }
        r
    });
    Some(
        col()
            .key("stats-session-links")
            .w_full()
            .min_w(0.0)
            .gap(4.0)
            .child(
                row()
                    .w_full()
                    .flex_wrap()
                    .gap(6.0)
                    .children(chips.collect::<Vec<_>>()),
            )
            .child(
                text(first.line.clone())
                    .font(ty::MONO_XS)
                    .color(v.t.muted_fg)
                    .truncate()
                    .help(first.line.clone()),
            ),
    )
}
