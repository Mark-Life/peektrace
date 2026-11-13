//! A tool call and its result as one card.

use peektrace_protocol::view::TimelineEvent;
use rune::{El, col, row, text};

use super::body::{code_block, diff_block, markdown};
use super::nodes::lane_row;
use crate::model::chat::{Lane, collapse_id};
use crate::model::format::fmt_k;
use crate::model::labels::token_label;
use crate::model::tools::{
    Body, ToolPane, ToolState, call_view, result_pane, tool_name_of, tool_state,
};
use crate::state::{Detail, Ui};
use crate::theme::{Theme, hex_or_grey, ty};
use crate::views::widgets::badge;

fn state_color(s: ToolState, t: &Theme) -> rune::Color {
    match s {
        ToolState::Completed => hex_or_grey("#34d399"),
        ToolState::Error => hex_or_grey("#f87171"),
        ToolState::Unanswered => t.muted_fg,
    }
}

fn section(label: &str, t: &Theme) -> El {
    text(label.to_uppercase()).font(ty::TINY).color(t.muted_fg)
}

fn pane(key: &str, p: &ToolPane, full: bool, t: &Theme) -> El {
    let body = match p {
        ToolPane::Code { code, language, .. } => code_block(key, code, language, full, t),
        ToolPane::Diff { diff, .. } => diff_block(diff, t),
        ToolPane::Prose { text: s, .. } => markdown(s, t),
    };
    col()
        .w_full()
        .gap(4.0)
        .child(section(p.label(), t))
        .child(body)
}

fn result(key: &str, e: &TimelineEvent, full: bool, t: &Theme) -> El {
    let body = match result_pane(e) {
        Body::Code { code, language } => code_block(key, &code, language, full, t),
        Body::Prose { text: s } => markdown(&s, t),
    };
    col()
        .w_full()
        .gap(4.0)
        .child(section("Result", t))
        .child(body)
}

pub fn tool_card(s: &Ui, d: &Detail, call: Option<usize>, res: Option<usize>, t: &Theme) -> El {
    let events = &d.a.events;
    let Some(pos) = call.or(res) else {
        return rune::el();
    };
    let id = collapse_id(pos);
    let open = s.open.contains(&id);
    let head_event = &events[pos];
    let name = tool_name_of(events, head_event, &d.tools).to_owned();
    let state = tool_state(events, head_event, &d.tools);
    let view = call.map(|c| call_view(&events[c]));
    let summary = view.as_ref().map_or_else(
        || crate::model::tools::one_line(&head_event.preview),
        |v| v.summary.clone(),
    );
    let tokens = format!(
        "{} → {}",
        call.map_or_else(|| "—".to_owned(), |c| token_label(&events[c], fmt_k)),
        res.map_or_else(|| "—".to_owned(), |r| token_label(&events[r], fmt_k))
    );
    let toggle = id.clone();
    let header = t
        .kit
        .interactive(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .px(12.0)
                .py(8.0)
                .child(
                    text(if open { "▾" } else { "›" })
                        .font(ty::XS)
                        .color(t.muted_fg),
                )
                .child(text(name).font(ty::XS_MEDIUM).color(t.fg))
                .child(badge(state.as_str(), state_color(state, t), t))
                .child(
                    text(summary)
                        .font(ty::MONO_XS)
                        .color(t.muted_fg)
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(text(tokens).font(ty::MONO_11).color(t.muted_fg)),
        )
        .role("button")
        .expanded(open)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.toggle(&toggle));
    let mut card = col()
        .key(id.clone())
        .w_full()
        .max_w(820.0)
        .radius(10.0)
        .border(1.0, t.border)
        .fill(t.card)
        .child(header);
    if open {
        let mut body = col().w_full().px(12.0).pb(12.0).gap(10.0);
        if let Some(v) = &view {
            for (i, p) in v.panes.iter().enumerate() {
                let key = format!("{id}:p{i}");
                body = body.child(pane(&key, p, s.full.contains(&key), t));
            }
        }
        if let Some(r) = res {
            let key = format!("{id}:result");
            body = body.child(result(&key, &events[r], s.full.contains(&key), t));
        }
        card = card.child(body);
    }
    lane_row(Lane::Agent, card)
}
