//! What fails and what costs time: two ranked tables whose rows open the
//! drill sheet, with the row's detail under it while its drill is open.

use peektrace_protocol::DrillTable;
use peektrace_protocol::stats::{FailRowView, TimeRowView};
use rune::{Color, El, col, row, text};

use super::causes::{Column, cell, head_row, shown};
use super::copy::prompt_button;
use super::links::links;
use super::{V, muted, outline_badge};
use crate::model::stats::rows::{backgrounded, co_present_note, time_badges, time_cells};
use crate::state::Ui;
use crate::theme::ty;

const FAIL_COLUMNS: [Column; 5] = [
    Column {
        head: "Where",
        w: 160.0,
        from: 0,
        right: false,
    },
    Column {
        head: "Sessions",
        w: 80.0,
        from: 0,
        right: true,
    },
    Column {
        head: "Calls",
        w: 72.0,
        from: 0,
        right: true,
    },
    Column {
        head: "Flagged",
        w: 72.0,
        from: 0,
        right: true,
    },
    Column {
        head: "Projects",
        w: 80.0,
        from: 2,
        right: true,
    },
];

const TIME_COLUMNS: [Column; 6] = [
    Column {
        head: "Time",
        w: 80.0,
        from: 0,
        right: true,
    },
    Column {
        head: "Share",
        w: 80.0,
        from: 0,
        right: true,
    },
    Column {
        head: "Runs",
        w: 64.0,
        from: 0,
        right: true,
    },
    Column {
        head: "In other jobs",
        w: 88.0,
        from: 2,
        right: true,
    },
    Column {
        head: "Median",
        w: 72.0,
        from: 1,
        right: true,
    },
    Column {
        head: "p95",
        w: 72.0,
        from: 1,
        right: true,
    },
];

fn selected(v: &V<'_>, table: DrillTable) -> Option<String> {
    v.s.stats
        .drill
        .as_ref()
        .filter(|(t, _)| *t == table)
        .map(|(_, k)| k.clone())
}

fn pressable(v: &V<'_>, line: El, key: String, table: DrillTable, label: &str, on: bool) -> El {
    let t = v.t;
    let row_key = key.clone();
    t.kit
        .interactive(line.fill(if on {
            t.muted.alpha(0.4)
        } else {
            Color::TRANSPARENT
        }))
        .key(row_key)
        .role("button")
        .label(label.to_owned())
        .focusable(true)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .on_press(move |s: &mut Ui| {
            let k = key.split_once(':').map_or("", |(_, k)| k).to_owned();
            s.open_drill(table, &k);
        })
}

fn detail_box(key: String, v: &V<'_>) -> El {
    col()
        .key(key)
        .w_full()
        .gap(8.0)
        .px(12.0)
        .py(10.0)
        .fill(v.t.muted.alpha(0.3))
        .border_sides([0.0, 0.0, 1.0, 0.0], v.t.border)
}

fn fail_row(v: &V<'_>, r: &FailRowView, on: bool) -> El {
    let t = v.t;
    let mut what = col().grow(1.0).min_w(0.0).max_w(420.0).gap(4.0).child(
        text(r.label.clone())
            .font(ty::SM_MEDIUM)
            .color(t.fg)
            .truncate(),
    );
    if let Some(d) = &r.detector {
        what = what.child(row().w_full().child(outline_badge(d.clone(), true, t)));
    }
    let values = [
        None,
        Some((crate::model::stats::format::n(r.sessions), t.fg)),
        Some((crate::model::stats::format::n(r.rows), t.fg)),
        Some((crate::model::stats::format::n(r.flagged), t.muted_fg)),
        Some((crate::model::stats::format::n(r.projects), t.muted_fg)),
    ];
    let mut line = row()
        .w_full()
        .px(8.0)
        .py(8.0)
        .gap(8.0)
        .items_start()
        .child(what);
    for (c, val) in FAIL_COLUMNS.iter().zip(values) {
        if !shown(c, v) {
            continue;
        }
        line = line.child(match val {
            Some(val) => cell(c, vec![val], None, t),
            None => col().w(c.w).child(muted(r.where_.clone(), t)),
        });
    }
    pressable(
        v,
        line,
        format!("fail-row:{}", r.key),
        DrillTable::Fail,
        &r.label,
        on,
    )
}

fn fail_detail(v: &V<'_>, r: &FailRowView, layer: &mut Vec<El>) -> El {
    let prompt = v.text.and_then(|x| x.prompts.of(DrillTable::Fail, &r.key));
    detail_box(format!("fail-detail:{}", r.key), v)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .items_start()
                .child(
                    text(r.note.clone().unwrap_or_default())
                        .font(ty::XS)
                        .color(v.t.fg)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .children(prompt_button(
                    v.s,
                    "fail-prompt",
                    format!("fail-prompt:{}", r.key),
                    &r.label,
                    prompt,
                    v.t,
                )),
        )
        .children(links(v, &format!("fail/{}", r.key), &r.samples, layer))
}

pub fn fails(v: &V<'_>, layer: &mut Vec<El>) -> El {
    let t = v.t;
    if v.r.fails.is_empty() {
        return text("Nothing failed in this window.")
            .font(ty::SM)
            .color(t.muted_fg)
            .key("fail-empty");
    }
    let sel = selected(v, DrillTable::Fail);
    let mut c = col()
        .key("fail-table")
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .child(head_row("What", &FAIL_COLUMNS, v));
    for r in &v.r.fails {
        let on = sel.as_deref() == Some(r.key.as_str());
        c = c.child(fail_row(v, r, on));
        if on {
            c = c.child(fail_detail(v, r, layer));
        }
    }
    c
}

fn time_row(v: &V<'_>, r: &TimeRowView, on: bool) -> El {
    let t = v.t;
    let job = row()
        .grow(1.0)
        .min_w(0.0)
        .flex_wrap()
        .gap(8.0)
        .center_y()
        .child(
            text(r.label.clone())
                .font(ty::SM_MEDIUM)
                .color(t.fg)
                .truncate()
                .shrink(),
        )
        .children(
            time_badges(r)
                .into_iter()
                .map(|b| outline_badge(b, false, t)),
        );
    let cells = time_cells(r);
    let colors = [t.fg, t.fg, t.fg, t.muted_fg, t.muted_fg, t.muted_fg];
    let mut line = row()
        .w_full()
        .px(8.0)
        .py(8.0)
        .gap(8.0)
        .items_start()
        .child(job);
    for ((c, val), color) in TIME_COLUMNS.iter().zip(cells).zip(colors) {
        if shown(c, v) {
            line = line.child(cell(c, vec![(val, color)], None, t));
        }
    }
    pressable(
        v,
        line,
        format!("time-row:{}", r.key),
        DrillTable::Time,
        &r.label,
        on,
    )
}

fn time_detail(v: &V<'_>, r: &TimeRowView, layer: &mut Vec<El>) -> El {
    let t = v.t;
    let prompt = v.text.and_then(|x| x.prompts.of(DrillTable::Time, &r.key));
    let note = v.text.and_then(|x| x.time_notes.get(&r.key));
    detail_box(format!("time-detail:{}", r.key), v)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .items_start()
                .child(muted(co_present_note(r), t).grow(1.0).min_w(0.0))
                .children(prompt_button(
                    v.s,
                    "time-prompt",
                    format!("time-prompt:{}", r.key),
                    &format!("{} time", r.label),
                    prompt,
                    t,
                )),
        )
        .children(note.map(|n| text(n.clone()).font(ty::XS).color(t.fg).wrap()))
        .children(backgrounded(r).map(|b| muted(b, t)))
        .children(links(v, &format!("time/{}", r.key), &r.samples, layer))
}

pub fn time(v: &V<'_>, layer: &mut Vec<El>) -> El {
    let t = v.t;
    if v.r.time.is_empty() {
        return text("No shell time measured in this window.")
            .font(ty::SM)
            .color(t.muted_fg)
            .key("time-empty");
    }
    let sel = selected(v, DrillTable::Time);
    let mut c = col()
        .key("time-table")
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .child(head_row("Job", &TIME_COLUMNS, v));
    for r in &v.r.time {
        let on = sel.as_deref() == Some(r.key.as_str());
        c = c.child(time_row(v, r, on));
        if on {
            c = c.child(time_detail(v, r, layer));
        }
    }
    c
}
