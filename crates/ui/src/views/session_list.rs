//! The session rail: search, agent facet, count line and virtual rows.

use std::borrow::Cow;

use peektrace_protocol::AgentId;
use peektrace_protocol::view::SessionHeader;
use rune::{El, GrowthSpec, ScrollSpec, Tx, col, el, memo, row, spacer, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Select, TextInput, glyphs};

use super::widgets::{badge, dot};
use crate::model::format::fmt_bytes;
use crate::model::list::{self, agent_label};
use crate::state::Ui;
use crate::theme::{EMERALD_500, Theme, agent_color, hex_or_grey, ty};

pub const RAIL_W: f32 = 320.0;
const ROW_H: f32 = 60.0;
const ROW_GAP: f32 = 4.0;
const ALL_AGENTS: &str = "All agents";

/// The rail's row key for a session id.
pub fn row_key(id: &str) -> String {
    format!("s:{id}")
}

fn stamp(h: &SessionHeader) -> String {
    let at = Some(h.updated_at.as_str())
        .filter(|s| !s.is_empty())
        .or(h.started_at.as_deref());
    list::fmt_stamp(at)
}

fn session_row(h: &SessionHeader, selected: bool, live: bool, focused: bool, t: &Theme) -> El {
    let id = h.id.clone();
    let mut line1 = row().w_full().gap(6.0).center_y();
    if live {
        line1 = line1.child(
            dot(8.0, hex_or_grey(EMERALD_500))
                .label("Live")
                .help("Live: wrote a line in the last 5 min"),
        );
    }
    line1 = line1
        .child(
            text(list::title_of(h))
                .font(ty::SM_MEDIUM)
                .color(t.fg)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            text(stamp(h))
                .font(ty::XS)
                .color(t.muted_fg)
                .help(list::stamp_title(h)),
        );
    let mut line2 = row().w_full().gap(6.0).center_y().child(badge(
        agent_label(h.agent),
        agent_color(h.agent),
        t,
    ));
    if let Some(src) = h.source_label.as_deref() {
        line2 = line2.child(badge(src, t.muted_fg, t));
    }
    line2 = line2.child(
        text(list::project_label(h).to_owned())
            .font(ty::XS)
            .color(t.muted_fg)
            .truncate()
            .max_w(130.0)
            .help(h.cwd.clone().unwrap_or_else(|| h.project.clone())),
    );
    if let Some(b) = list::visible_branch(h.git_branch.as_deref()) {
        line2 = line2.child(
            text(b.to_owned())
                .font(ty::MONO_XS)
                .color(t.muted_fg)
                .truncate()
                .max_w(80.0),
        );
    }
    let detail = format!(
        "{} · {}",
        fmt_bytes(f64_of(h.size_bytes)),
        h.model.as_deref().unwrap_or(&h.id)
    );
    line2 = line2.child(spacer()).child(
        text(format!("{} msgs", h.message_count))
            .font(ty::XS)
            .color(t.muted_fg)
            .help(detail),
    );
    let (border, fill) = if selected {
        (t.primary.alpha(0.4), t.primary.alpha(0.1))
    } else if focused {
        (t.primary.alpha(0.3), t.card)
    } else {
        (t.border, t.card)
    };
    t.kit
        .interactive(
            col()
                .w_full()
                .h(ROW_H)
                .px(12.0)
                .py(8.0)
                .gap(4.0)
                .radius(10.0)
                .border(1.0, border)
                .fill(fill)
                .child(line1)
                .child(line2),
        )
        .role("link")
        .label(list::title_of(h))
        .focusable(true)
        .on_press_tx(move |s: &mut Ui, tx: &mut Tx| {
            s.select(&id, None);
            tx.set_title(s.title());
        })
}

#[expect(clippy::cast_precision_loss, reason = "file sizes fit f64")]
const fn f64_of(n: u64) -> f64 {
    n as f64
}

fn agent_select(s: &Ui, t: &Theme, layer: &mut Vec<El>) -> El {
    let agents = s
        .rail
        .listing
        .as_ref()
        .map(|l| list::agents_present(&l.headers))
        .unwrap_or_default();
    let options: Vec<&str> = std::iter::once(ALL_AGENTS)
        .chain(agents.iter().map(|a| agent_label(*a)))
        .collect();
    let current = s.rail.filters.agent.map_or(ALL_AGENTS, agent_label);
    Select::new("agent-facet", &options)
        .width(130.0)
        .current(Some(current))
        .label("Agent")
        .open(s.agent_open)
        .on_toggle(|s: &mut Ui| s.agent_open = !s.agent_open)
        .on_close(|s: &mut Ui| s.agent_open = false)
        .on_choose(|s: &mut Ui, picked: &str, _tx: &mut Tx| {
            let agent = AgentId::ALL.into_iter().find(|a| agent_label(*a) == picked);
            s.set_agent(agent);
        })
        .build(&t.kit, layer)
}

fn controls(s: &Ui, t: &Theme, layer: &mut Vec<El>) -> El {
    let search = TextInput::new(&s.query)
        .placeholder("Search by title…")
        .leading_icon(&glyphs::SEARCH)
        .clear_button()
        .on_edit(|s: &mut Ui| &mut s.query)
        .on_change(|s: &mut Ui, tx: &mut Tx| {
            let clock = tx.clock();
            s.query_edited(clock, tx);
        })
        .build(&t.kit)
        .key("search")
        .grow(1.0)
        .min_w(0.0);
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(search)
        .child(agent_select(s, t, layer))
}

/// Why the list may be stale: live refresh misses some changes.
fn watch_problems(s: &Ui, t: &Theme) -> Option<El> {
    (!s.watch_problems.is_empty()).then(|| {
        col()
            .key("watch-problems")
            .w_full()
            .radius(8.0)
            .border(1.0, t.destructive)
            .px(10.0)
            .py(8.0)
            .gap(4.0)
            .children(s.watch_problems.iter().map(|p| {
                text(format!("Warning: {p}"))
                    .font(ty::XS)
                    .color(t.destructive)
                    .wrap()
                    .selectable(true)
            }))
    })
}

fn count_line(s: &Ui, t: &Theme) -> El {
    let n = s.rail.rows.len();
    let mut r = row().w_full().center_y().justify_between().child(
        text(format!("{n} session{}", if n == 1 { "" } else { "s" }))
            .font(ty::XS)
            .color(t.muted_fg)
            .key("count"),
    );
    let active = s.active_filters();
    if active > 0 {
        r = r.child(
            Button::new(format!("Clear {active}"))
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Ui| s.reset_filters())
                .build(&t.kit)
                .key("clear-filters"),
        );
    }
    r
}

fn empty(title: &str, body: &str, t: &Theme, reset: bool) -> El {
    let mut c = col()
        .w_full()
        .py(32.0)
        .gap(6.0)
        .items_center()
        .child(text(title.to_owned()).font(ty::SM_MEDIUM).color(t.fg))
        .child(
            text(body.to_owned())
                .font(ty::XS)
                .color(t.muted_fg)
                .wrap()
                .align(rune::Align::Center),
        );
    if reset {
        c = c.child(
            Button::new("Reset filters")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Ui| s.reset_filters())
                .build(&t.kit)
                .key("reset-filters"),
        );
    }
    c
}

fn rows(s: &Ui, t: &Theme) -> El {
    let theme = t.clone();
    col()
        .key("rail-rows")
        .on_key(crate::rail_key)
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            s.rail.rows.len(),
            ROW_H + ROW_GAP,
            |s: &Ui, i| {
                s.rail
                    .rows
                    .get(i)
                    .and_then(|&h| s.rail.header(h))
                    .map_or(Cow::Borrowed("s:?"), |h| Cow::Owned(row_key(&h.id)))
            },
            move |s: &Ui, i| {
                let Some(h) = s.rail.rows.get(i).and_then(|&h| s.rail.header(h)) else {
                    return el();
                };
                let selected = s.selected.as_ref().is_some_and(|x| x.id == h.id);
                let focused = s.cursor.as_deref() == Some(h.id.as_str());
                let live = list::is_live(h, s.now_ms);
                let version = (s.rail.rev, selected, focused, live, theme.dark);
                memo(row_key(&h.id), version, || {
                    col()
                        .w_full()
                        .pb(ROW_GAP)
                        .child(session_row(h, selected, live, focused, &theme))
                })
            },
        )
        .virtual_rows_version((s.rail.rev, s.rail.rows.len()))
}

pub fn rail(s: &Ui, t: &Theme, layer: &mut Vec<El>) -> El {
    let body = match (&s.rail.listing, &s.list_error) {
        (None, Some(e)) => empty("Could not list sessions", &e.to_string(), t, false),
        (None, None) => empty("Loading sessions…", "", t, false),
        (Some(l), _) if l.headers.is_empty() => empty(
            "No sessions found",
            "No transcripts in the agent folders yet.",
            t,
            false,
        ),
        (Some(_), _) if s.rail.rows.is_empty() => empty(
            "No matching sessions",
            "No session matches the current search and filters.",
            t,
            true,
        ),
        (Some(_), _) => rows(s, t),
    };
    col()
        .key("rail")
        .w(RAIL_W)
        .h_full()
        .gap(8.0)
        .child(controls(s, t, layer))
        .children(watch_problems(s, t))
        .child(count_line(s, t))
        .child(
            col()
                .key("rail-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(body),
        )
}
