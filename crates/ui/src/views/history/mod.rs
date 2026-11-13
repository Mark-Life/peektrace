//! The full history in chat layout: heading, toolbar and one virtual row
//! per chat node, keyed by `chat_node_keys` so rows keep their measured
//! height across live appends.

mod body;
mod nodes;
mod run;
mod tool_card;

use std::borrow::Cow;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use rune::{El, GrowthSpec, Tx, col, el, memo, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

pub use body::{CUT_BYTES, PLAIN_BYTES, head_of};

use crate::model::chat::{ChatNode, collapse_id};
use crate::model::row::hash_chat_event;
use crate::model::tools::tool_name_of;
use crate::state::{Detail, Ui};
use crate::theme::{RED_400, Theme, hex_or_grey, ty};
use crate::views::summary::{HISTORY_KEY, SUMMARY_KEY};

pub const ROWS_KEY: &str = "history-rows";
const ROW_ESTIMATE: f32 = 72.0;

fn ids_of(n: &ChatNode) -> Vec<usize> {
    match n {
        ChatNode::Message(p) | ChatNode::Band(p) | ChatNode::Notice(p) => vec![*p],
        ChatNode::Tool { call, result } => call.iter().chain(result).copied().collect(),
        ChatNode::Run(items) => items.iter().map(|i| i.pos).collect(),
        _ => Vec::new(),
    }
}

/// Everything a row's look depends on. Rows that show events hash the
/// fields they render, so a live re-analysis keeps unchanged rows; turn
/// rules and the dumb-zone band read the whole analysis and use its revision.
fn row_version(s: &Ui, d: &Detail, i: usize, dark: bool) -> u64 {
    let mut h = DefaultHasher::new();
    (&d.id, dark).hash(&mut h);
    let node = &d.plan[i];
    node.hash(&mut h);
    let ids = ids_of(node);
    if ids.is_empty() {
        d.rev.hash(&mut h);
    }
    if let ChatNode::Tool { call, result } = node
        && let Some(head) = call.or(*result)
    {
        tool_name_of(&d.a.events, &d.a.events[head], &d.tools).hash(&mut h);
    }
    for pos in ids {
        hash_chat_event(&d.a.events[pos], &mut h);
        let id = collapse_id(pos);
        (
            s.open.contains(&id),
            s.raw.contains(&id),
            s.full.contains(&id),
        )
            .hash(&mut h);
        for suffix in [":result", ":p0", ":p1", ":p2", ":p3"] {
            s.full.contains(&format!("{id}{suffix}")).hash(&mut h);
        }
    }
    if let ChatNode::Run(items) = node {
        let first = items.first().map_or(0, |i| i.pos);
        (s.runs_shown.contains(&first), s.all_open()).hash(&mut h);
    }
    h.finish()
}

pub fn node_view(s: &Ui, d: &Detail, i: usize, t: &Theme) -> El {
    let events = &d.a.events;
    let inner = match &d.plan[i] {
        ChatNode::Message(p) => nodes::message(s, *p, &events[*p], t),
        ChatNode::Band(p) => nodes::band(s, *p, &events[*p], t),
        ChatNode::Notice(p) => nodes::notice(s, *p, &events[*p], t),
        ChatNode::Tool { call, result } => tool_card::tool_card(s, d, *call, *result, t),
        ChatNode::Run(items) => run::run(s, d, items, t),
        ChatNode::Turn(n) => nodes::turn_rule(&d.a, *n, t),
        ChatNode::DumbZone => nodes::dumb_zone(&d.a),
    };
    col().w_full().py(4.0).child(inner)
}

fn toolbar(t: &Theme) -> El {
    let button = |label: &str| {
        Button::new(label)
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
    };
    row()
        .key("history-toolbar")
        .w_full()
        .gap(4.0)
        .py(4.0)
        .center_y()
        .child(
            button("Expand all")
                .on_press(|s: &mut Ui| s.expand_all())
                .build(&t.kit)
                .key("expand-all"),
        )
        .child(
            button("Collapse all")
                .on_press(|s: &mut Ui| s.collapse_all())
                .build(&t.kit)
                .key("collapse-all"),
        )
        .child(rune::spacer())
        .child(
            button("Back to summary ↑")
                .on_press_tx(|_s: &mut Ui, tx: &mut Tx| tx.scroll_into_view(SUMMARY_KEY))
                .build(&t.kit)
                .key("back-to-summary"),
        )
}

pub fn heading(d: &Detail, t: &Theme) -> El {
    let a = &d.a;
    col()
        .key(HISTORY_KEY)
        .w_full()
        .gap(4.0)
        .pt(8.0)
        .child(
            text(format!(
                "Full history · {} events · {}/{} turns in dumb zone",
                a.events.len(),
                a.dumb_zone_turns,
                a.turn_count
            ))
            .font(ty::BASE_SEMI)
            .color(t.fg),
        )
        .child(
            text("Secrets are redacted by default.")
                .font(ty::XS)
                .color(hex_or_grey(RED_400)),
        )
        .child(toolbar(t))
}

pub fn rows(d: &Detail, t: &Theme) -> El {
    let theme = t.clone();
    let rev = d.rev;
    col()
        .key(ROWS_KEY)
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            d.plan.len(),
            ROW_ESTIMATE,
            |s: &Ui, i| {
                s.detail
                    .as_ref()
                    .and_then(|d| d.keys.get(i))
                    .map_or(Cow::Borrowed("?"), |k| Cow::Borrowed(k.as_str()))
            },
            move |s: &Ui, i| {
                let Some(d) = &s.detail else { return el() };
                let Some(key) = d.keys.get(i) else {
                    return el();
                };
                memo(key.clone(), row_version(s, d, i, theme.dark), || {
                    node_view(s, d, i, &theme)
                })
            },
        )
        .virtual_rows_version((rev, d.plan.len()))
}
