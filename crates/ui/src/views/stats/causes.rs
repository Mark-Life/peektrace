//! Avoidable retries: named causes, what each cost in round trips, and
//! its fix. Rows keep the order core ranked them in.

use peektrace_protocol::DrillTable;
use peektrace_protocol::stats::CauseRowView;
use peektrace_protocol::stats_text::EMPTY_CAUSES;
use rune::{Align, Color, El, col, row, text};

use super::copy::{copy_button, prompt_button};
use super::links::links;
use super::{V, amber, amber_text, mono, muted, outline_badge, secondary_badge, small_button};
use crate::model::stats::rows::{
    cause_badge, cause_cells, cause_learned, cause_named, cause_narrow, cause_pairs, cause_split,
    match_lines,
};
use crate::state::Ui;
use crate::theme::{Theme, ty};

const NO_RULE: &str = "no rule to suggest";

/// A column: header, width, shown from which breakpoint (0 always, 1 md, 2 lg).
pub struct Column {
    pub head: &'static str,
    pub w: f32,
    pub from: u8,
    pub right: bool,
}

const COLUMNS: [Column; 6] = [
    Column {
        head: "Hits",
        w: 72.0,
        from: 1,
        right: true,
    },
    Column {
        head: "Retries",
        w: 96.0,
        from: 1,
        right: true,
    },
    Column {
        head: "Hits main / sub",
        w: 128.0,
        from: 2,
        right: true,
    },
    Column {
        head: "Retry time",
        w: 96.0,
        from: 1,
        right: true,
    },
    Column {
        head: "Context re-read",
        w: 128.0,
        from: 2,
        right: true,
    },
    Column {
        head: "Rule",
        w: 200.0,
        from: 1,
        right: false,
    },
];

pub fn shown(c: &Column, v: &V<'_>) -> bool {
    match c.from {
        0 => true,
        1 => v.bp.md,
        _ => v.bp.lg,
    }
}

/// The header row of a table: the first column grows.
pub fn head_row(first: &str, cols: &[Column], v: &V<'_>) -> El {
    let t = v.t;
    row()
        .w_full()
        .h(36.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .child(
            text(first.to_owned())
                .font(ty::XS_MEDIUM)
                .color(t.muted_fg)
                .grow(1.0)
                .min_w(0.0),
        )
        .children(cols.iter().filter(|c| shown(c, v)).map(|c| {
            text(c.head)
                .font(ty::XS_MEDIUM)
                .color(t.muted_fg)
                .w(c.w)
                .align(if c.right { Align::End } else { Align::Start })
        }))
}

/// One numeric cell: `lines` stacked, the first in mono SM.
pub fn cell(c: &Column, lines: Vec<(String, Color)>, sub: Option<String>, t: &Theme) -> El {
    let align = if c.right { Align::End } else { Align::Start };
    let mut e = col().w(c.w).gap(2.0);
    if c.right {
        e = e.items_end();
    }
    e = e.children(
        lines
            .into_iter()
            .map(|(s, color)| text(s).font(ty::MONO_SM).color(color).align(align)),
    );
    e.children(sub.map(|s| muted(s, t).align(align)))
}

/// The rule that should have prevented the cause, cut to one line; the
/// full line is its tooltip and heads the open row's rule check.
fn rule_badge(key: &str, line: &str) -> El {
    let badge = row()
        .key(format!("cause-rule:{key}"))
        .shrink()
        .px(8.0)
        .py(2.0)
        .radius(6.0)
        .border(1.0, amber().alpha(0.4))
        .fill(amber().alpha(0.1))
        .help(line.to_owned())
        .child(
            text(line.to_owned())
                .font(ty::XS_MEDIUM)
                .color(amber_text())
                .truncate()
                .min_w(0.0),
        );
    row().w_full().min_w(0.0).child(badge)
}

fn rule_line<'a>(v: &V<'a>, row: &CauseRowView) -> Option<&'a str> {
    v.text?.rule_lines.get(&row.key).map(String::as_str)
}

fn cause_cell(v: &V<'_>, r: &CauseRowView) -> El {
    let t = v.t;
    let mut c = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(
            text(r.label.clone())
                .font(ty::SM_MEDIUM)
                .color(t.fg)
                .truncate()
                .key(format!("cause-toggle:{}", r.key)),
        )
        .child(
            row()
                .w_full()
                .gap(4.0)
                .child(outline_badge(cause_badge(&r.key), true, t))
                .child(secondary_badge(r.kind.clone(), t)),
        );
    if !v.bp.md {
        let mut narrow = col()
            .key(format!("cause-narrow:{}", r.key))
            .w_full()
            .gap(4.0)
            .child(mono(cause_narrow(r), t));
        if let Some(l) = rule_line(v, r) {
            narrow = narrow.child(rule_badge(&r.key, l));
        }
        c = c.child(narrow);
    }
    c
}

fn cause_row(v: &V<'_>, r: &CauseRowView, open: bool, selected: bool) -> El {
    let t = v.t;
    let cells = cause_cells(r);
    let muted_c = t.muted_fg;
    let values: [Vec<(String, Color)>; 5] = [
        vec![(cells.hits, t.fg)],
        vec![(cells.retries, t.fg)],
        vec![(cells.hits_split, muted_c)],
        vec![(cells.retry_time, muted_c)],
        vec![(cells.ctx, muted_c)],
    ];
    let mut line = row()
        .w_full()
        .px(8.0)
        .py(8.0)
        .gap(8.0)
        .items_start()
        .child(cause_cell(v, r));
    for (i, (c, vals)) in COLUMNS.iter().zip(values).enumerate() {
        if shown(c, v) {
            let sub = (i == 1).then(|| cells.retry_sessions.clone());
            line = line.child(cell(c, vals, sub, t));
        }
    }
    if shown(&COLUMNS[5], v) {
        let rule = rule_line(v, r).map_or_else(
            || text("—").font(ty::XS).color(t.muted_fg),
            |l| rule_badge(&r.key, l),
        );
        line = line.child(col().w(COLUMNS[5].w).child(rule));
    }
    let key = r.key.clone();
    t.kit
        .interactive(line.fill(if open || selected {
            t.muted.alpha(0.4)
        } else {
            Color::TRANSPARENT
        }))
        .key(format!("cause-row:{}", r.key))
        .role("button")
        .label(r.label.clone())
        .expanded(open)
        .focusable(true)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .on_press(move |s: &mut Ui| s.toggle_cause(&key))
}

fn detail(v: &V<'_>, r: &CauseRowView, layer: &mut Vec<El>) -> El {
    let t = v.t;
    let suggested = v.text.and_then(|x| x.suggested_rule.get(&r.key));
    let mut top = row().w_full().flex_wrap().gap(8.0).items_start().child(
        text(r.fix.clone().unwrap_or_else(|| NO_RULE.to_owned()))
            .font(ty::XS)
            .color(t.fg)
            .wrap()
            .grow(1.0)
            .min_w(0.0),
    );
    if let Some(rule) = suggested {
        top = top.child(copy_button(
            v.s,
            "cause-copy",
            format!("cause-copy:{}", r.key),
            "Copy rule",
            rule,
            None,
            t,
        ));
    }
    let prompt = v.text.and_then(|x| x.prompts.of(DrillTable::Cause, &r.key));
    top = top.children(prompt_button(
        v.s,
        "cause-prompt",
        format!("cause-prompt:{}", r.key),
        &r.label,
        prompt,
        t,
    ));
    let key = r.key.clone();
    top = top.child(
        small_button("All pairs", false, t)
            .on_press(move |s: &mut Ui| s.open_drill(DrillTable::Cause, &key))
            .build(&t.kit)
            .key("cause-drill"),
    );
    let mut c = col()
        .key(format!("cause-detail:{}", r.key))
        .w_full()
        .gap(8.0)
        .px(12.0)
        .py(10.0)
        .fill(t.muted.alpha(0.3))
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .child(top);
    if let (Some(rule), Some(line)) = (&r.rule, rule_line(v, r)) {
        c = c.child(
            col()
                .w_full()
                .gap(4.0)
                .padding(8.0)
                .radius(6.0)
                .border(1.0, amber().alpha(0.4))
                .fill(amber().alpha(0.05))
                .child(
                    text(format!("{line} — {}", rule.advice))
                        .font(ty::XS)
                        .color(t.fg)
                        .wrap(),
                )
                .children(match_lines(r).into_iter().map(|m| mono(m, t))),
        );
    }
    c = c.child(mono(cause_split(r), t).key(format!("cause-split:{}", r.key)));
    c = c.children(cause_learned(r).map(|l| mono(l, t)));
    c = c.children(cause_named(r).map(|l| mono(l, t)));
    let pairs = cause_pairs(r);
    if !pairs.is_empty() {
        c = c.child(
            col()
                .key("cause-pairs")
                .gap(2.0)
                .children(pairs.into_iter().map(|p| mono(p, t))),
        );
    }
    c.children(links(v, &format!("cause/{}", r.key), &r.samples, layer))
}

pub fn table(v: &V<'_>, layer: &mut Vec<El>) -> El {
    let t = v.t;
    if v.r.causes.is_empty() {
        return text(EMPTY_CAUSES)
            .font(ty::SM)
            .color(t.muted_fg)
            .key("cause-empty");
    }
    let drill_key =
        v.s.stats
            .drill
            .as_ref()
            .filter(|(tb, _)| *tb == DrillTable::Cause)
            .map(|(_, k)| k.as_str());
    let mut c = col()
        .key("cause-table")
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .child(head_row("Cause", &COLUMNS, v));
    for r in &v.r.causes {
        let open = v.s.stats.open_cause.as_deref() == Some(r.key.as_str());
        c = c.child(cause_row(v, r, open, drill_key == Some(r.key.as_str())));
        if open {
            c = c.child(detail(v, r, layer));
        }
    }
    c
}
