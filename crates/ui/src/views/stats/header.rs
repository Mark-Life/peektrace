//! What the report covers, and the knobs that change it: window, agent,
//! rescan, the whole-report prompt and the caveats.

use peektrace_protocol::AgentId;
use rune::{El, Tx, col, icon, row, spacer, text};
use rune_kit::{Collapsible, Select};

use super::copy::prompt_button;
use super::{REFRESH, SHIELD_CHECK, V, muted, small_button};
use crate::model::stats::rows::{DAY_OPTIONS, caveats_line, corpus_line, window_line};
use crate::state::Ui;
use crate::theme::ty;

const ALL_AGENTS: &str = "All agents";

fn day_label(d: u32) -> String {
    format!("{d} days")
}

fn controls(v: &V<'_>, rescanning: bool, layer: &mut Vec<El>) -> El {
    let (s, t) = (v.s, v.t);
    let days: Vec<String> = DAY_OPTIONS.iter().map(|d| day_label(*d)).collect();
    let current = day_label(s.stats.scope.days);
    let days = Select::new("stats-days", &days)
        .width(128.0)
        .current(Some(current.as_str()))
        .label("Window")
        .open(s.stats.days_open)
        .on_toggle(|s: &mut Ui| s.stats.days_open = !s.stats.days_open)
        .on_close(|s: &mut Ui| s.stats.days_open = false)
        .on_choose(|s: &mut Ui, picked: &str, tx: &mut Tx| {
            let days = DAY_OPTIONS
                .into_iter()
                .find(|d| day_label(*d) == picked)
                .unwrap_or(s.stats.scope.days);
            s.set_stats_scope(days, s.stats.scope.agent, tx.clock());
        })
        .build(&t.kit, layer)
        .key("stats-days");
    let agents: Vec<&str> = std::iter::once(ALL_AGENTS)
        .chain(AgentId::ALL.iter().map(|a| a.as_str()))
        .collect();
    let agent = Select::new("stats-agent", &agents)
        .width(144.0)
        .current(Some(
            s.stats.scope.agent.map_or(ALL_AGENTS, AgentId::as_str),
        ))
        .label("Agent")
        .open(s.stats.agent_open)
        .on_toggle(|s: &mut Ui| s.stats.agent_open = !s.stats.agent_open)
        .on_close(|s: &mut Ui| s.stats.agent_open = false)
        .on_choose(|s: &mut Ui, picked: &str, tx: &mut Tx| {
            s.set_stats_scope(s.stats.scope.days, AgentId::parse(picked), tx.clock());
        })
        .build(&t.kit, layer)
        .key("stats-agent");
    let rescan = small_button(
        if rescanning {
            "Rescanning…"
        } else {
            "Rescan"
        },
        false,
        t,
    )
    .icon(&REFRESH)
    .disabled(rescanning)
    .on_press_tx(|s: &mut Ui, tx: &mut Tx| s.stats_ask(tx.clock()))
    .build(&t.kit)
    .key("stats-rescan");
    let prompt = prompt_button(
        s,
        "stats-prompt",
        "stats-prompt".to_owned(),
        "this report",
        v.text.map(|x| x.prompts.report.as_str()),
        t,
    );
    row()
        .flex_wrap()
        .gap(8.0)
        .center_y()
        .child(days)
        .child(agent)
        .child(rescan)
        .children(prompt)
}

pub fn header(v: &V<'_>, rescanning: bool, layer: &mut Vec<El>) -> El {
    let (s, r, t) = (v.s, v.r, v.t);
    let mut list = col().key("stats-caveats").w_full().gap(4.0).pl(16.0);
    list = list
        .children(r.caveats.iter().map(|c| muted(format!("• {c}"), t)))
        .children(
            r.skipped
                .iter()
                .map(|p| muted(format!("• skipped: {p}"), t)),
        );
    let caveats = Collapsible::new(caveats_line(r), s.stats.caveats_open)
        .ghost()
        .on_toggle(|s: &mut Ui| s.stats.caveats_open = !s.stats.caveats_open)
        .child(list)
        .build(&t.kit)
        .key("stats-caveats-toggle");
    col()
        .key("stats-header")
        .w_full()
        .gap(12.0)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(12.0)
                .center_y()
                .child(
                    text(corpus_line(&r.corpus))
                        .font(ty::MONO_SM)
                        .color(t.fg)
                        .wrap()
                        .key("stats-corpus"),
                )
                .child(spacer())
                .child(controls(v, rescanning, layer)),
        )
        .child(muted(window_line(r, v.generated), t).key("stats-window"))
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(icon(&SHIELD_CHECK, 14.0).color(t.muted_fg))
                .child(muted(
                    "Command text, error signatures and cluster labels are redacted before they are stored or shown.",
                    t,
                )),
        )
        .child(caveats)
}
