//! A run of context chips: one centred cluster, chips open in place.

use rune::{El, col, row, text};

use super::nodes::{event_body, lane_row};
use crate::model::chat::{
    Lane, RunItem, Segment, chip_label, chip_window, collapse_id, segments_of,
};
use crate::model::format::fmt_k;
use crate::model::labels::{event_tokens, token_label};
use crate::state::{Detail, Ui};
use crate::theme::{Theme, ty};

const TOKEN_CHIP_FLOOR: f64 = 500.0;

fn chip(d: &Detail, item: RunItem, t: &Theme) -> El {
    let e = &d.a.events[item.pos];
    let id = collapse_id(item.pos);
    let mut r = row()
        .h(24.0)
        .px(8.0)
        .gap(6.0)
        .radius(12.0)
        .center_y()
        .border(1.0, t.border)
        .child(
            text(chip_label(e))
                .font(ty::MONO_11)
                .color(t.fg)
                .truncate()
                .max_w(224.0),
        );
    if event_tokens(e).0 >= TOKEN_CHIP_FLOOR {
        r = r.child(
            text(token_label(e, fmt_k))
                .font(ty::MONO_11)
                .color(t.muted_fg),
        );
    }
    let toggle = id.clone();
    t.kit
        .interactive(r)
        .key(id)
        .role("button")
        .expanded(false)
        .focusable(true)
        .help(
            e.attachment_type
                .clone()
                .unwrap_or_else(|| e.kind.as_str().to_owned()),
        )
        .on_press(move |s: &mut Ui| s.toggle(&toggle))
}

fn card(s: &Ui, d: &Detail, item: RunItem, t: &Theme) -> El {
    let e = &d.a.events[item.pos];
    let id = collapse_id(item.pos);
    let toggle = id.clone();
    let mut head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .px(10.0)
        .py(6.0)
        .fill(t.primary.alpha(0.1))
        .child(
            text(chip_label(e))
                .font(ty::MONO_XS)
                .color(t.fg)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        );
    if let Some(ty_) = &e.attachment_type {
        head = head.child(text(ty_.clone()).font(ty::MONO_11).color(t.muted_fg));
    }
    head = head.child(
        text(format!("{} tok ▴", token_label(e, fmt_k)))
            .font(ty::MONO_11)
            .color(t.muted_fg),
    );
    let head = t
        .kit
        .interactive(head)
        .key(id.clone())
        .role("button")
        .expanded(true)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.toggle(&toggle));
    col()
        .w_full()
        .radius(8.0)
        .border(1.0, t.primary.alpha(0.4))
        .fill(t.bg)
        .child(head)
        .child(
            col()
                .w_full()
                .padding(10.0)
                .child(event_body(&id, e, s.full.contains(&id), t)),
        )
}

fn more(label: String, first: usize, t: &Theme) -> El {
    t.kit
        .interactive(
            row()
                .h(24.0)
                .px(8.0)
                .radius(12.0)
                .center_y()
                .border(1.0, t.border)
                .child(text(label).font(ty::MONO_11).color(t.muted_fg)),
        )
        .role("button")
        .focusable(true)
        .on_press(move |s: &mut Ui| {
            s.runs_shown.insert(first);
        })
}

pub fn run(s: &Ui, d: &Detail, items: &[RunItem], t: &Theme) -> El {
    let first = items.first().map_or(0, |i| i.pos);
    let opened = |i: &RunItem| s.open.contains(&collapse_id(i.pos));
    let window = chip_window(items, s.all_open(), s.runs_shown.contains(&first), opened);
    let (tokens, measured) = items.iter().fold((0.0, true), |(sum, all), i| {
        let (n, m) = event_tokens(&d.a.events[i.pos]);
        (sum + n, all && m)
    });
    let summary = format!(
        "+{} items loaded · {}{} tok",
        items.len(),
        if measured { "" } else { "~" },
        fmt_k(tokens)
    );
    let mut c = col()
        .key(format!("run{first}"))
        .w_full()
        .gap(6.0)
        .items_center()
        .child(text(summary).font(ty::TINY).color(t.muted_fg));
    let segments = segments_of(&window.shown, opened);
    let mut extras = Vec::new();
    if window.folded > 0 {
        extras.push(
            more(format!("{} unchanged", window.folded), first, t)
                .help("Same as earlier in this session"),
        );
    }
    if window.hidden > 0 {
        extras.push(more(format!("+{} more", window.hidden), first, t));
    }
    let last_is_chips = matches!(segments.last(), Some(Segment::Chips(_)));
    let n = segments.len();
    for (i, seg) in segments.into_iter().enumerate() {
        match seg {
            Segment::Card(item) => c = c.child(card(s, d, item, t)),
            Segment::Chips(chips) => {
                let mut r = row()
                    .w_full()
                    .gap(6.0)
                    .flex_wrap()
                    .justify_center()
                    .children(chips.into_iter().map(|it| chip(d, it, t)));
                if i == n - 1 {
                    r = r.children(std::mem::take(&mut extras));
                }
                c = c.child(r);
            }
        }
    }
    if !last_is_chips && !extras.is_empty() {
        c = c.child(row().w_full().gap(6.0).justify_center().children(extras));
    }
    lane_row(Lane::Center, c)
}
