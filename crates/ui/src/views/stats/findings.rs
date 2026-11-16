//! What a cold start leads with: the rows that carry a fix, cheapest fix
//! first, each in its own table's unit.

use peektrace_protocol::DrillTable;
use peektrace_protocol::stats::{SampleRefView, StatsReportView};
use peektrace_protocol::stats_text::{EMPTY_FINDINGS, SUGGESTED_NOTE, SUGGESTED_TITLE};
use rune::{El, col, row, spacer, text};

use super::copy::{copy_button, prompt_button};
use super::links::links;
use super::{LIST_TREE, V, amber, muted, outline_badge, secondary_badge, small_button};
use crate::model::stats::Finding;
use crate::model::stats::rows::{VISIBLE_FINDINGS, show_more, table_name};
use crate::state::Ui;
use crate::theme::ty;

const fn source_label(t: DrillTable) -> &'static str {
    match t {
        DrillTable::Cause => "retries",
        DrillTable::Fail => "fails",
        _ => "time",
    }
}

/// The samples of the row a finding came from.
pub fn samples_of<'a>(r: &'a StatsReportView, table: DrillTable, key: &str) -> &'a [SampleRefView] {
    let found = match table {
        DrillTable::Cause => r.causes.iter().find(|c| c.key == key).map(|c| &c.samples),
        DrillTable::Fail => r.fails.iter().find(|c| c.key == key).map(|c| &c.samples),
        DrillTable::Time => r.time.iter().find(|c| c.key == key).map(|c| &c.samples),
        _ => None,
    };
    found.map_or(&[], Vec::as_slice)
}

fn card(v: &V<'_>, f: &Finding, layer: &mut Vec<El>) -> El {
    let t = v.t;
    let id = format!("{}:{}", table_name(f.table), f.key);
    let mut head = row()
        .w_full()
        .flex_wrap()
        .gap(8.0)
        .items_start()
        .child(
            text(f.title.clone())
                .font(ty::SM_MEDIUM)
                .color(t.fg)
                .wrap()
                .min_w(0.0),
        )
        .child(secondary_badge(source_label(f.table), t));
    if let Some(d) = &f.detector {
        head = head.child(outline_badge(d.clone(), true, t));
    }
    head = head.child(spacer()).child(
        text(f.impact.clone())
            .font(ty::MONO_SM)
            .color(t.fg)
            .wrap()
            .shrink()
            .key("stats-finding-impact"),
    );
    let samples = samples_of(v.r, f.table, &f.key);
    let mut actions = row().w_full().flex_wrap().gap(8.0).items_end();
    if let Some(l) = links(v, &format!("finding/{id}"), samples, layer) {
        actions = actions.child(l);
    }
    if let Some(rule) = &f.rule {
        actions = actions.child(copy_button(
            v.s,
            "finding-copy",
            format!("finding-copy:{id}"),
            "Copy rule",
            rule,
            None,
            t,
        ));
    }
    let prompt = v.text.and_then(|x| x.prompts.of(f.table, &f.key));
    actions = actions.children(prompt_button(
        v.s,
        "finding-prompt",
        format!("finding-prompt:{id}"),
        &f.title,
        prompt,
        t,
    ));
    let (table, key) = (f.table, f.key.clone());
    actions = actions.child(spacer()).child(
        small_button("All calls", true, t)
            .icon(&LIST_TREE)
            .on_press(move |s: &mut Ui| s.open_drill(table, &key))
            .build(&t.kit)
            .key("finding-drill"),
    );
    let flagged = f.detector.is_some();
    col()
        .key(format!("stats-finding:{id}"))
        .w_full()
        .min_w(0.0)
        .gap(8.0)
        .padding(12.0)
        .radius(8.0)
        .border(
            1.0,
            if flagged {
                amber().alpha(0.4)
            } else {
                t.border
            },
        )
        .fill(if flagged {
            amber().alpha(0.05)
        } else {
            rune::Color::TRANSPARENT
        })
        .child(head)
        .child(muted(f.note.clone(), t))
        .child(
            text(f.spread.clone())
                .font(ty::MONO_XS)
                .color(t.muted_fg)
                .wrap(),
        )
        .child(actions)
}

fn suggested(v: &V<'_>, rules: &[String]) -> Option<El> {
    let t = v.t;
    (!rules.is_empty()).then(|| {
        let all: Vec<String> = rules.iter().map(|r| format!("- {r}")).collect();
        col()
            .key("stats-suggested-rules")
            .w_full()
            .gap(8.0)
            .padding(12.0)
            .radius(8.0)
            .border(1.0, t.border)
            .child(
                row()
                    .w_full()
                    .flex_wrap()
                    .gap(8.0)
                    .center_y()
                    .child(text(SUGGESTED_TITLE).font(ty::SM_MEDIUM).color(t.fg))
                    .child(muted(SUGGESTED_NOTE, t))
                    .child(spacer())
                    .child(copy_button(
                        v.s,
                        "stats-copy-all",
                        "stats-copy-all".to_owned(),
                        "Copy all suggested rules",
                        &all.join("\n"),
                        None,
                        t,
                    )),
            )
            .child(
                col()
                    .gap(2.0)
                    .pl(8.0)
                    .children(rules.iter().map(|r| muted(format!("• {r}"), t))),
            )
    })
}

fn empty(body: &str, v: &V<'_>) -> El {
    text(body.to_owned())
        .font(ty::SM)
        .color(v.t.muted_fg)
        .wrap()
        .key("stats-no-findings")
}

pub fn findings(v: &V<'_>, layer: &mut Vec<El>) -> El {
    let Some(text) = v.text else {
        return empty("This server sent no finding text.", v);
    };
    let all = &text.findings;
    if all.is_empty() {
        return empty(EMPTY_FINDINGS, v);
    }
    let shown = if v.s.stats.show_all {
        all.len()
    } else {
        all.len().min(VISIBLE_FINDINGS)
    };
    let cards: Vec<El> = all[..shown].iter().map(|f| card(v, f, layer)).collect();
    let columns = if v.bp.lg { 2 } else { 1 };
    let mut grid = col().w_full().gap(12.0);
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let mut line = row().w_full().gap(12.0).items_start();
        for _ in 0..columns {
            line = line.child(cards.next().unwrap_or_else(rune::el).grow(1.0).min_w(0.0));
        }
        grid = grid.child(line);
    }
    let mut c = col().key("stats-findings").w_full().gap(12.0).child(grid);
    if !v.s.stats.show_all
        && let Some(more) = show_more(all.len())
    {
        c = c.child(
            row().child(
                small_button(more, false, v.t)
                    .on_press(|s: &mut Ui| s.stats.show_all = true)
                    .build(&v.t.kit)
                    .key("stats-show-all"),
            ),
        );
    }
    c.children(suggested(v, &text.suggested))
}
