//! The detail summary: jump button, parent note and the verdict card.

use peektrace_protocol::view::AnalyzedSession;
use rune::{Align, El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use super::widgets::{badge, card, chip};
use crate::model::format::{Zone, fmt, fmt_k, fmt_pct, zone_of};
use crate::model::labels::{analyzed_title, cost_summary, provider_label};
use crate::state::{Detail, Ui};
use crate::theme::{RED_400, Theme, hex_or_grey, ty, zone_fill, zone_text};

pub const HISTORY_KEY: &str = "history-heading";
pub const SUMMARY_KEY: &str = "summary";

fn frac(a: &AnalyzedSession) -> f64 {
    if a.context_window == 0.0 {
        0.0
    } else {
        a.peak_context_tokens / a.context_window
    }
}

#[expect(clippy::cast_possible_truncation, reason = "a percentage of a bar")]
fn gauge(a: &AnalyzedSession, zone: Zone, t: &Theme) -> El {
    let f = frac(a);
    let fill = f.clamp(0.0, 1.0) as f32;
    let tick = (a.dumb_zone_fraction.clamp(0.0, 1.0)) as f32;
    let bar = row()
        .key("peak-gauge")
        .w_full()
        .h(16.0)
        .radius(8.0)
        .fill(t.muted)
        .child(el().w_frac(fill).h_full().radius(8.0).fill(zone_fill(zone)))
        .child(
            el().absolute(0.0, 0.0)
                .w_frac(tick)
                .h_full()
                .border_sides([0.0, 2.0, 0.0, 0.0], t.fg.alpha(0.7))
                .help(format!(
                    "dumb zone {}%",
                    fmt_round(a.dumb_zone_fraction * 100.0)
                )),
        );
    col().w_full().gap(4.0).child(bar).child(
        row()
            .w_full()
            .justify_between()
            .child(
                text(format!("peak {}", fmt(a.peak_context_tokens)))
                    .font(ty::XS)
                    .color(t.muted_fg)
                    .key("peak-tokens"),
            )
            .child(
                text(format!("{} of {}", fmt_pct(f), fmt(a.context_window)))
                    .font(ty::XS)
                    .color(t.muted_fg),
            ),
    )
}

fn fmt_round(n: f64) -> String {
    crate::model::jsnum::to_js_string(crate::model::jsnum::js_round(n))
}

fn tile(key: &str, label: &str, value: El, t: &Theme) -> El {
    col()
        .key(key.to_owned())
        .min_w(0.0)
        .radius(8.0)
        .border(1.0, t.border)
        .fill(t.card)
        .px(12.0)
        .py(8.0)
        .gap(2.0)
        .child(
            text(label.to_uppercase())
                .font(ty::XS)
                .color(t.muted_fg)
                .truncate(),
        )
        .child(value)
}

fn value(s: impl Into<String>, t: &Theme) -> El {
    text(s.into()).font(ty::SM).color(t.fg).wrap_anywhere()
}

fn tiles(a: &AnalyzedSession, zone: Zone, cols: usize, t: &Theme) -> El {
    let mut items = vec![
        tile(
            "tile-peak",
            "Peak context",
            text(fmt(a.peak_context_tokens))
                .font(ty::SM)
                .color(zone_text(zone)),
            t,
        ),
        tile(
            "tile-turns",
            "Turns",
            value(
                format!(
                    "{} · {} user · {} tools",
                    a.turn_count, a.user_message_count, a.tool_call_count
                ),
                t,
            ),
            t,
        ),
        tile(
            "tile-tax",
            "System + tools tax",
            value(fmt(a.system_overhead_tokens), t),
            t,
        ),
        tile(
            "tile-output",
            "Output",
            value(fmt(a.total_output_tokens), t),
            t,
        ),
    ];
    if let Some(cost) = &a.cost {
        let c = cost_summary(cost, provider_label(&a.provider));
        let mut v = c.total;
        if !c.notes.is_empty() {
            v = format!("{v} · {}", c.notes.join(" · "));
        }
        let mut el = value(v, t).key("session-cost");
        if cost.source == peektrace_protocol::view::CostSource::Estimate {
            el = el.help("Estimate at list price");
        }
        items.push(tile("tile-cost", "Est. cost", el, t));
    }
    items.push(tile(
        "tile-dwell",
        "Dumb-zone dwell",
        value(format!("{}/{}", a.dumb_zone_turns, a.turn_count), t),
        t,
    ));
    let cache = if a.peak_context_tokens == 0.0 {
        0.0
    } else {
        a.peak_cache_read_tokens / a.peak_context_tokens
    };
    items.push(tile("tile-cache", "Cache", value(fmt_pct(cache), t), t));
    let mut grid = col().w_full().gap(8.0);
    let mut it = items.into_iter().peekable();
    while it.peek().is_some() {
        let mut r = row().w_full().gap(8.0).items_stretch();
        for _ in 0..cols {
            r = r.child(it.next().unwrap_or_else(el).grow(1.0).w(0.0));
        }
        grid = grid.child(r);
    }
    grid
}

fn jump_chip(
    key: &str,
    label: String,
    color: rune::Color,
    hint: String,
    row_key: Option<String>,
    t: &Theme,
) -> El {
    // The name first, then the jump hint, as the inspector reads it out.
    let c = chip(label.clone(), color, t)
        .key(key.to_owned())
        .label(format!("{label}, {hint}"))
        .help(hint);
    let Some(target) = row_key else { return c };
    t.kit
        .interactive(c)
        .role("button")
        .focusable(true)
        .on_press_tx(move |_s: &mut Ui, tx: &mut Tx| tx.scroll_into_view(target.clone()))
}

fn chips(d: &Detail, t: &Theme) -> El {
    let a = &d.a;
    let muted = t.muted_fg;
    let mut r = row().w_full().gap(6.0).flex_wrap().child(
        chip(
            format!(
                "window {}{}",
                fmt_k(a.context_window),
                if a.context_window_inferred {
                    " (assumed)"
                } else {
                    ""
                }
            ),
            muted,
            t,
        )
        .key("chip-window"),
    );
    if let Ok(c) = usize::try_from(a.dumb_zone_cross_turn) {
        let turn = c + 1;
        let target =
            crate::model::chat::plan_index_of_turn(&d.plan, turn).map(|i| d.keys[i].clone());
        r = r.child(jump_chip(
            "chip-cross",
            format!("entered dumb zone @ turn {turn}"),
            hex_or_grey(RED_400),
            "jump to that turn in history".to_owned(),
            target,
            t,
        ));
    } else {
        r = r.child(chip("stayed in smart zone", hex_or_grey("#34d399"), t).key("chip-smart"));
    }
    let n = a.compaction_turns.len();
    if n > 0 {
        let target = crate::model::chat::plan_index_of_compaction(&a.events, &d.plan)
            .map(|i| d.keys[i].clone());
        r = r.child(jump_chip(
            "chip-compactions",
            format!("{n} compaction(s)"),
            muted,
            "jump to the first in history".into(),
            target,
            t,
        ));
    }
    let n = a.subagents.len();
    if n > 0 {
        let first = a.subagents.iter().find_map(|s| s.tool_use_id.as_deref());
        let target = first
            .and_then(|id| crate::model::chat::plan_index_of_call(&a.events, &d.plan, id))
            .map(|i| d.keys[i].clone());
        r = r.child(jump_chip(
            "chip-subagents",
            format!("{n} subagent(s)"),
            muted,
            "jump to the first in history".into(),
            target,
            t,
        ));
    }
    let js = crate::model::jsnum::to_js_string;
    if a.web_search_requests > 0.0 {
        r = r.child(
            chip(
                format!("{} web search(es)", js(a.web_search_requests)),
                muted,
                t,
            )
            .key("chip-search"),
        );
    }
    if a.web_fetch_requests > 0.0 {
        r = r.child(
            chip(
                format!("{} web fetch(es)", js(a.web_fetch_requests)),
                muted,
                t,
            )
            .key("chip-fetch"),
        );
    }
    if let Some(b) = a.git_branch.as_deref().filter(|b| !b.is_empty()) {
        r = r.child(chip(b.to_owned(), t.fg, t).key("chip-branch"));
    }
    r
}

/// Columns of the tile grid at a card width.
pub const fn tile_columns(width: f32) -> usize {
    if width >= 896.0 {
        6
    } else if width >= 512.0 {
        3
    } else {
        2
    }
}

pub fn verdict(d: &Detail, width: f32, t: &Theme) -> El {
    let a = &d.a;
    let zone = zone_of(frac(a), a.dumb_zone_fraction);
    let id8: String = a.session_id.chars().take(8).collect();
    let meta = format!(
        "{id8} · {} · {}",
        a.models.join(", "),
        a.cwd.as_deref().unwrap_or_default()
    );
    let header = row()
        .w_full()
        .gap(8.0)
        .items_start()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    text(analyzed_title(a))
                        .font(ty::LG_SEMI)
                        .color(t.fg)
                        .wrap()
                        .key("verdict-title"),
                )
                .child(text(meta).font(ty::MONO_XS).color(t.muted_fg).truncate()),
        )
        .child(badge(provider_label(&a.provider), t.fg, t))
        .child(badge(zone.health(), zone_text(zone), t).key("health"));
    card(t)
        .key("verdict")
        .child(header)
        .child(gauge(a, zone, t))
        .child(tiles(a, zone, tile_columns(width), t))
        .child(chips(d, t))
}

pub fn jump_to_history(t: &Theme) -> El {
    Button::new("Jump to history ↓")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press_tx(|_s: &mut Ui, tx: &mut Tx| tx.scroll_into_view(HISTORY_KEY))
        .build(&t.kit)
        .key("jump-history")
}

pub fn parent_note(parent: &str, t: &Theme) -> El {
    let id = parent.to_owned();
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(format!("Subagent of {parent}"))
                .font(ty::XS)
                .color(t.muted_fg)
                .align(Align::Start),
        )
        .child(
            Button::new("Open parent session")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press_tx(move |s: &mut Ui, tx: &mut Tx| {
                    s.select(&id, None);
                    tx.set_title(s.title());
                })
                .build(&t.kit)
                .key("open-parent"),
        )
}

#[cfg(test)]
mod tests;
