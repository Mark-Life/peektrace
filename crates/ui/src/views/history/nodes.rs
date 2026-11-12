//! Bubbles, thinking pills, bands, notices, turn rules and the dumb-zone band.

use std::fmt::Write as _;

use peektrace_protocol::view::{AnalyzedSession, EventKind, TimelineEvent};
use rune::{Color, El, col, rich, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use super::body::{code_block, markdown, plain};
use crate::model::chat::{Lane, collapse_id, lane_of, needs_clamp};
use crate::model::format::{fmt, fmt_k, fmt_pct, zone_of};
use crate::model::jsnum::{js_round, to_fixed, to_js_string};
use crate::model::labels::{event_tokens, thinking_note, token_label};
use crate::model::tools::body_view;
use crate::state::Ui;
use crate::theme::{Theme, hex_or_grey, ty, zone_text};
use crate::views::widgets::badge;

/// Bubble width in its lane.
const LANE_W: f32 = 720.0;
/// A clamped bubble shows about this many points of its body.
const CLAMP_H: f32 = 28.0 * 20.0;

pub fn fmt_thinking_ms(ms: f64) -> String {
    if ms < 1000.0 {
        return format!("{}ms", to_js_string(js_round(ms)));
    }
    let s = ms / 1000.0;
    if s < 10.0 {
        format!("{}s", to_fixed(s, 1))
    } else {
        format!("{}s", to_js_string(js_round(s)))
    }
}

fn ghost(label: &str, key: String, t: &Theme, f: impl Fn(&mut Ui) + 'static) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(f)
        .build(&t.kit)
        .key(key)
}

fn toggle_set(
    set: fn(&mut Ui) -> &mut std::collections::HashSet<String>,
    id: String,
) -> impl Fn(&mut Ui) {
    move |s: &mut Ui| {
        let set = set(s);
        if !set.remove(&id) {
            set.insert(id.clone());
        }
    }
}

/// The row a lane sits in: user right, agent left, centre centred.
pub fn lane_row(lane: Lane, child: El) -> El {
    let r = row().w_full();
    match lane {
        Lane::User => r.justify_end(),
        Lane::Agent => r.justify_start(),
        Lane::Center => r.justify_center(),
    }
    .child(child)
}

/// A non-tool body: a file's content as code, the rest as Markdown.
pub fn event_body(key: &str, e: &TimelineEvent, full: bool, t: &Theme) -> El {
    match body_view(e) {
        Some(language) => code_block(key, &e.body, language, full, t),
        None => markdown(&e.body, t),
    }
}

pub fn message(s: &Ui, pos: usize, e: &TimelineEvent, t: &Theme) -> El {
    if e.kind == EventKind::AssistantThinking {
        return thinking(s, pos, e, t);
    }
    let id = collapse_id(pos);
    let lane = lane_of(e);
    let open = s.open.contains(&id);
    let raw = s.raw.contains(&id);
    let clamps = needs_clamp(e);
    let who = if e.kind == EventKind::UserPrompt {
        "user"
    } else {
        "assistant"
    };
    let mut meta = row()
        .gap(6.0)
        .center_y()
        .child(text(who).font(ty::XS).color(t.muted_fg));
    if let Some(src) = &e.prompt_source {
        meta = meta.child(badge(src, t.muted_fg, t));
    }
    if e.tokens_est != 0.0 {
        meta = meta.child(
            text(format!("· {} tok", token_label(e, fmt)))
                .font(ty::XS)
                .color(t.muted_fg),
        );
    }
    if e.sidechain() {
        meta = meta.child(badge("sidechain", t.muted_fg, t));
    }
    if clamps && !raw {
        let label = if open { "Show less" } else { "Show all" };
        let key = format!("{id}:clamp");
        meta = meta.child(ghost(
            label,
            key,
            t,
            toggle_set(|s| &mut s.open, id.clone()),
        ));
    }
    let raw_label = if raw { "Markdown" } else { "Raw" };
    meta = meta.child(ghost(
        raw_label,
        format!("{id}:raw"),
        t,
        toggle_set(|s| &mut s.raw, id.clone()),
    ));
    let surface = if lane == Lane::User {
        t.primary.alpha(0.08)
    } else {
        t.card
    };
    let border = if lane == Lane::User {
        t.primary.alpha(0.5)
    } else {
        t.border
    };
    let body = if raw {
        plain(&e.body, t)
    } else {
        markdown(&e.body, t)
    };
    let mut bubble = col()
        .w_full()
        .radius(14.0)
        .border(1.0, border)
        .fill(surface)
        .px(14.0)
        .py(10.0)
        .child(body);
    if clamps && !open && !raw {
        bubble = bubble.max_h(CLAMP_H).clip_fade(0.0, 0.0, 0.0, 48.0);
    }
    let meta = if lane == Lane::User {
        row().w_full().justify_end().child(meta)
    } else {
        meta
    };
    lane_row(
        lane,
        col()
            .key(id)
            .max_w(LANE_W)
            .w_full()
            .gap(4.0)
            .child(meta)
            .child(bubble),
    )
}

fn thinking(s: &Ui, pos: usize, e: &TimelineEvent, t: &Theme) -> El {
    let id = collapse_id(pos);
    let open = s.open.contains(&id);
    let mut label = "thinking".to_owned();
    if event_tokens(e).0 != 0.0 {
        let _ = write!(label, " · {} tok", token_label(e, fmt_k));
    }
    if let Some(ms) = e.thinking_duration_ms {
        let _ = write!(label, " · {}", fmt_thinking_ms(ms));
    }
    let toggle = id.clone();
    let pill = t
        .kit
        .interactive(
            row()
                .gap(6.0)
                .center_y()
                .radius(14.0)
                .border(1.0, t.border)
                .border_dash(4.0, 3.0)
                .px(12.0)
                .py(6.0)
                .child(
                    text(format!("{} {label}", if open { "▾" } else { "›" }))
                        .font(ty::XS.italic())
                        .color(t.muted_fg),
                ),
        )
        .role("button")
        .expanded(open)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.toggle(&toggle));
    let mut c = col()
        .key(id)
        .max_w(LANE_W)
        .w_full()
        .gap(4.0)
        .child(row().child(pill));
    if open {
        let body = if e.body.trim().is_empty() {
            text(thinking_note(e)).font(ty::XS).color(t.muted_fg).wrap()
        } else {
            markdown(&e.body, t)
        };
        c = c.child(
            col()
                .w_full()
                .radius(8.0)
                .fill(t.muted.alpha(0.3))
                .px(12.0)
                .py(8.0)
                .child(body),
        );
    }
    lane_row(Lane::Agent, c)
}

fn band_label(e: &TimelineEvent) -> String {
    match e.kind {
        EventKind::Compaction => {
            "context compacted — everything above was replaced by this summary".to_owned()
        }
        EventKind::Summary => "rolling summary".to_owned(),
        _ => e.title.to_lowercase(),
    }
}

pub fn band(s: &Ui, pos: usize, e: &TimelineEvent, t: &Theme) -> El {
    let id = collapse_id(pos);
    let open = s.open.contains(&id);
    let compaction = e.kind == EventKind::Compaction;
    let amber = hex_or_grey("#f59e0b");
    let toggle = id.clone();
    let head = t
        .kit
        .interactive(
            row().w_full().justify_center().child(
                text(format!("{} · {}", band_label(e), token_label(e, fmt_k)))
                    .font(ty::XS)
                    .color(if compaction { amber } else { t.fg }),
            ),
        )
        .role("button")
        .expanded(open)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.toggle(&toggle));
    let mut c = col()
        .key(id.clone())
        .w_full()
        .max_w(if compaction { 10_000.0 } else { 672.0 })
        .radius(8.0)
        .border(
            1.0,
            if compaction {
                amber.alpha(0.4)
            } else {
                t.border
            },
        )
        .fill(if compaction {
            amber.alpha(0.1)
        } else {
            t.muted.alpha(0.3)
        })
        .px(12.0)
        .py(8.0)
        .gap(4.0)
        .child(head)
        .child(
            text(e.preview.clone())
                .font(ty::XS)
                .color(t.muted_fg)
                .truncate(),
        );
    if !compaction {
        c = c.border_dash(4.0, 3.0);
    }
    if open {
        c = c.child(markdown(&e.body, t));
    }
    lane_row(Lane::Center, c)
}

pub fn notice(s: &Ui, pos: usize, e: &TimelineEvent, t: &Theme) -> El {
    let id = collapse_id(pos);
    let open = s.open.contains(&id);
    let full = s.full.contains(&id);
    let toggle = id.clone();
    let mut head = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            text(if open { "▾" } else { "›" })
                .font(ty::XS)
                .color(t.muted_fg),
        )
        .child(text(e.title.clone()).font(ty::XS_MEDIUM).color(t.fg))
        .child(
            text(e.preview.clone())
                .font(ty::XS)
                .color(t.muted_fg)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        );
    if let Some(src) = &e.prompt_source {
        head = head.child(badge(src, t.muted_fg, t));
    }
    head = head.child(
        text(format!("{} tok", token_label(e, fmt_k)))
            .font(ty::MONO_11)
            .color(t.muted_fg),
    );
    let head = t
        .kit
        .interactive(head)
        .role("button")
        .expanded(open)
        .focusable(true)
        .on_press(move |s: &mut Ui| s.toggle(&toggle));
    let mut c = col()
        .key(id.clone())
        .w_full()
        .max_w(672.0)
        .radius(8.0)
        .border(1.0, t.border)
        .border_dash(4.0, 3.0)
        .fill(t.muted.alpha(0.3))
        .px(12.0)
        .py(8.0)
        .gap(6.0)
        .child(head);
    if open {
        c = c.child(event_body(&id, e, full, t));
    }
    lane_row(Lane::Center, c)
}

fn rule_line(t: &Theme) -> El {
    rune::el().h(1.0).grow(1.0).fill(t.border)
}

pub fn turn_rule(a: &AnalyzedSession, n: usize, t: &Theme) -> El {
    let Some(turn) = n.checked_sub(1).and_then(|i| a.turns.get(i)) else {
        return rune::el().key(format!("turn{n}")).h(1.0);
    };
    let frac = if a.context_window > 0.0 {
        turn.context_tokens / a.context_window
    } else {
        0.0
    };
    let zone = zone_of(frac, a.dumb_zone_fraction);
    let effort = turn
        .effort
        .as_deref()
        .map(|e| format!(" · {e} effort"))
        .unwrap_or_default();
    let head = format!(
        "turn {n} · {}{effort} · ctx {} ",
        turn.model,
        fmt_k(turn.context_tokens)
    );
    let pct = format!("({})", fmt_pct(frac));
    let mut tail = format!(" · out {}", fmt_k(turn.output_tokens));
    if let Some(th) = turn.thinking_tokens.filter(|v| *v != 0.0 && !v.is_nan()) {
        let _ = write!(tail, " ({} thinking)", fmt_k(th));
    }
    let plural = |n: f64, one: &str, many: &str| {
        format!(
            "{} {}",
            to_js_string(n),
            if (n - 1.0).abs() < f64::EPSILON {
                one
            } else {
                many
            }
        )
    };
    if let Some(w) = turn
        .web_search_requests
        .filter(|v| *v != 0.0 && !v.is_nan())
    {
        let _ = write!(tail, " · {}", plural(w, "web search", "web searches"));
    }
    if let Some(w) = turn.web_fetch_requests.filter(|v| *v != 0.0 && !v.is_nan()) {
        let _ = write!(tail, " · {}", plural(w, "web fetch", "web fetches"));
    }
    let content = format!("{head}{pct}{tail}");
    let start = head.len();
    let spans = vec![rune::Span::new(start, start + pct.len()).color(zone_text(zone))];
    row()
        .key(format!("turn{n}"))
        .w_full()
        .gap(8.0)
        .center_y()
        .py(4.0)
        .role("heading")
        .child(rule_line(t))
        .child(rich(content, spans).font(ty::MONO_11).color(t.muted_fg))
        .child(rule_line(t))
}

pub fn dumb_zone(a: &AnalyzedSession) -> El {
    let f = a.dumb_zone_fraction;
    let red: Color = hex_or_grey("#ef4444");
    row()
        .key("dumbzone")
        .w_full()
        .justify_center()
        .fill(red.alpha(0.15))
        .px(12.0)
        .py(6.0)
        .child(
            text(format!(
                "entered dumb zone — {}% ({} tok) crossed at turn {}",
                to_js_string(js_round(f * 100.0)),
                fmt(f * a.context_window),
                a.dumb_zone_cross_turn + 1
            ))
            .font(ty::XS_MEDIUM)
            .color(hex_or_grey("#fca5a5")),
        )
}
