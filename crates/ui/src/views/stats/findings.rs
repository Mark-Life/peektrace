//! What a cold start leads with: where the spend goes, then the causes a
//! fix would still save, each in its own unit. Causes a rule already stopped
//! sit in a collapsed "working" list.

use peektrace_protocol::DrillTable;
use peektrace_protocol::stats::SampleRefView;
use peektrace_protocol::stats_text::{
    EMPTY_FINDINGS, SUGGESTED_NOTE, SUGGESTED_TITLE, WORKING_NOTE, WORKING_TITLE,
};
use peektrace_protocol::wire::stats::{SampleRef, TranscriptKind};
use rune::{El, col, row, spacer, text};
use rune_kit::Collapsible;

use super::copy::{copy_button, prompt_button};
use super::links::links;
use super::{
    LIST_TREE, V, amber, emerald, emerald_text, muted, outline_badge, secondary_badge, small_button,
};
use crate::model::stats::Finding;
use crate::model::stats::rows::{VISIBLE_FINDINGS, show_more, table_name};
use crate::state::Ui;
use crate::theme::ty;

const fn source_label(t: Option<DrillTable>) -> &'static str {
    match t {
        Some(DrillTable::Cause) => "retries",
        Some(DrillTable::Fail) => "fails",
        Some(_) => "time",
        None => "cost",
    }
}

/// A finding's sessions in the shape the link chips read.
pub fn sample_views(samples: &[SampleRef]) -> Vec<SampleRefView> {
    samples
        .iter()
        .map(|s| SampleRefView {
            sid: s.sid.clone(),
            agent: s.agent,
            project: s.project.clone(),
            kind: match s.kind {
                TranscriptKind::Main => "main",
                TranscriptKind::Subagent => "subagent",
                TranscriptKind::WorkflowAgent => "workflow-agent",
            }
            .to_owned(),
            parent: s.parent.clone(),
            seq: s.seq,
            tool_use_id: s.tool_use_id.clone(),
            ts: s.ts.clone(),
            line: s.line.clone(),
        })
        .collect()
}

/// How a card is drawn: a live finding, or a cause its rule stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Live,
    Fixed,
}

fn card(v: &V<'_>, f: &Finding, look: Look, layer: &mut Vec<El>) -> El {
    let t = v.t;
    let id = format!("{}:{}", f.table.map_or("cost", table_name), f.key);
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
    if look == Look::Fixed {
        head = head.child(
            text("fixed")
                .font(ty::XS_MEDIUM)
                .color(emerald_text())
                .key("stats-working-badge"),
        );
    }
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
    let samples = sample_views(&f.samples);
    let mut actions = row().w_full().flex_wrap().gap(8.0).items_end();
    if let Some(l) = links(v, &format!("finding/{id}"), &samples, layer) {
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
    if let Some(table) = f.table {
        let prompt = v.text.and_then(|x| x.prompts.of(table, &f.key));
        actions = actions.children(prompt_button(
            v.s,
            "finding-prompt",
            format!("finding-prompt:{id}"),
            &f.title,
            prompt,
            t,
        ));
        let key = f.key.clone();
        actions = actions.child(spacer()).child(
            small_button("All calls", true, t)
                .icon(&LIST_TREE)
                .on_press(move |s: &mut Ui| s.open_drill(table, &key))
                .build(&t.kit)
                .key("finding-drill"),
        );
    }
    let flagged = f.detector.is_some() && look == Look::Live;
    let edge = if look == Look::Fixed {
        emerald().alpha(0.4)
    } else if flagged {
        amber().alpha(0.4)
    } else {
        t.border
    };
    col()
        .key(format!("stats-finding:{id}"))
        .w_full()
        .min_w(0.0)
        .gap(8.0)
        .padding(12.0)
        .radius(8.0)
        .border(1.0, edge)
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
    let cards: Vec<El> = all[..shown]
        .iter()
        .map(|f| card(v, f, Look::Live, layer))
        .collect();
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
    c.children(working(v, &text.working, layer))
        .children(suggested(v, &text.suggested))
}

/// Causes a rule already stopped, collapsed under one line.
fn working(v: &V<'_>, all: &[Finding], layer: &mut Vec<El>) -> Option<El> {
    if all.is_empty() {
        return None;
    }
    let t = v.t;
    let cards = col()
        .key("stats-working")
        .w_full()
        .gap(12.0)
        .pt(8.0)
        .child(muted(WORKING_NOTE, t))
        .children(all.iter().map(|f| card(v, f, Look::Fixed, layer)));
    Some(
        Collapsible::new(
            format!("{WORKING_TITLE} ({})", all.len()),
            v.s.stats.working_open,
        )
        .ghost()
        .on_toggle(|s: &mut Ui| s.stats.working_open = !s.stats.working_open)
        .child(cards)
        .build(&t.kit)
        .key("stats-working-toggle"),
    )
}
