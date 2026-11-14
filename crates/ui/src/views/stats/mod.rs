//! The Stats screen: which named causes cost retries, what fails and what
//! costs time, as the inspector's Stats route draws them.

mod causes;
mod copy;
mod drill;
mod evidence;
mod fails_time;
mod findings;
mod header;
mod links;
mod pending;

use rune::{Color, Cx, El, Icon, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use peektrace_protocol::stats::{SampleRefView, StatsReportView};

use crate::model::stats::StatsText;
use crate::model::stats::chips::{SessionTitles, session_of, title_key};
use crate::source::LoadError;
use crate::state::Ui;
use crate::theme::{Theme, hex_or_grey, ty};

/// The column's breakpoints: `md` and `lg`.
pub const MD: f32 = 768.0;
pub const LG: f32 = 1024.0;
const AMBER_500: &str = "#f59e0b";
const AMBER_400: &str = "#fbbf24";
const SKY_500: &str = "#0ea5e9";

pub static ARROW_UP_RIGHT: Icon = Icon {
    name: "stats-arrow-up-right",
    view: 24.0,
    body: r#"<path d="M7 7h10v10M7 17 17 7" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
pub static LIST_TREE: Icon = Icon {
    name: "stats-list-tree",
    view: 24.0,
    body: r#"<path d="M21 12h-8M21 6H8M21 18h-8M3 6v4c0 1.1.9 2 2 2h3M3 10v6c0 1.1.9 2 2 2h3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
pub static SHIELD_CHECK: Icon = Icon {
    name: "stats-shield-check",
    view: 24.0,
    body: r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1zM9 12l2 2 4-4" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
pub static REFRESH: Icon = Icon {
    name: "stats-refresh",
    view: 24.0,
    body: r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8M21 3v5h-5M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16M8 16H3v5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub fn amber() -> Color {
    hex_or_grey(AMBER_500)
}

pub fn amber_text() -> Color {
    hex_or_grey(AMBER_400)
}

pub fn sky() -> Color {
    hex_or_grey(SKY_500)
}

/// The width breakpoints the column is at.
#[derive(Debug, Clone, Copy)]
pub struct Bp {
    pub md: bool,
    pub lg: bool,
}

impl Bp {
    pub fn of(w: f32) -> Self {
        Self {
            md: w >= MD,
            lg: w >= LG,
        }
    }
}

pub fn muted(s: impl Into<String>, t: &Theme) -> El {
    text(s.into()).font(ty::XS).color(t.muted_fg).wrap()
}

pub fn mono(s: impl Into<String>, t: &Theme) -> El {
    text(s.into()).font(ty::MONO_XS).color(t.muted_fg).wrap()
}

/// An outlined badge, `mono` for ids.
pub fn outline_badge(label: impl Into<String>, mono: bool, t: &Theme) -> El {
    row()
        .h(20.0)
        .px(8.0)
        .radius(6.0)
        .center_y()
        .border(1.0, t.border)
        .child(
            text(label.into())
                .font(if mono { ty::MONO_XS } else { ty::XS_MEDIUM })
                .color(t.fg),
        )
}

pub fn secondary_badge(label: impl Into<String>, t: &Theme) -> El {
    row()
        .h(20.0)
        .px(8.0)
        .radius(6.0)
        .center_y()
        .fill(t.muted)
        .child(text(label.into()).font(ty::XS_MEDIUM).color(t.fg))
}

/// A small button: outline or ghost, 24 tall.
pub fn small_button(label: impl Into<String>, ghost: bool, t: &Theme) -> Button {
    let _ = t;
    Button::new(label)
        .variant(if ghost {
            ButtonVariant::Ghost
        } else {
            ButtonVariant::Outline
        })
        .control_size(ControlSize::Regular)
}

/// A muted filled block standing in for content that is loading.
pub fn skeleton(w: Option<f32>, h: f32, t: &Theme) -> El {
    let b = el().h(h).radius(6.0).fill(t.muted);
    match w {
        Some(w) => b.w(w).max_w(f32::MAX),
        None => b.w_full(),
    }
}

fn section(key: &str, title: &str, by: &str, body: Vec<El>, t: &Theme) -> El {
    col()
        .key(key.to_owned())
        .w_full()
        .gap(12.0)
        .child(
            row()
                .flex_wrap()
                .gap(8.0)
                .items_end()
                .child(text(title.to_owned()).font(ty::BASE_SEMI).color(t.fg))
                .child(
                    text(format!("ranked by {by}"))
                        .font(ty::XS)
                        .color(t.muted_fg),
                ),
        )
        .children(body)
}

pub fn error_view(
    e: &LoadError,
    retrying: bool,
    retry: fn(&mut Ui, &mut rune::Tx),
    t: &Theme,
) -> El {
    let title = if matches!(e, LoadError::Api(_)) {
        "Something went wrong"
    } else {
        "Request failed"
    };
    col()
        .key("result-error")
        .w_full()
        .gap(8.0)
        .py(24.0)
        .items_center()
        .child(text(title).font(ty::SM_MEDIUM).color(t.fg))
        .child(text("The request failed.").font(ty::SM).color(t.muted_fg))
        .child(
            muted(e.to_string(), t)
                .align(rune::Align::Center)
                .max_w(560.0),
        )
        .child(
            small_button(if retrying { "Retrying…" } else { "Retry" }, false, t)
                .disabled(retrying)
                .on_press_tx(retry)
                .build(&t.kit)
                .key("result-retry"),
        )
}

/// What every stats view reads.
pub struct V<'a> {
    pub s: &'a Ui,
    pub r: &'a StatsReportView,
    pub text: Option<&'a StatsText>,
    /// `generatedAt` in local time.
    pub generated: &'a str,
    pub bp: Bp,
    pub titles: &'a SessionTitles,
    pub t: &'a Theme,
}

/// Session titles from the listing the rail already holds.
pub fn titles_of(s: &Ui) -> SessionTitles {
    s.rail
        .listing
        .iter()
        .flat_map(|l| &l.headers)
        .filter_map(|h| Some((title_key(h.agent, &h.id), h.title.clone()?)))
        .collect()
}

/// The one title a sample's chip can use.
pub fn title_for(s: &Ui, sample: &SampleRefView) -> SessionTitles {
    let id = session_of(sample);
    s.rail
        .listing
        .iter()
        .flat_map(|l| &l.headers)
        .find(|h| h.agent == sample.agent && h.id == id)
        .and_then(|h| Some((title_key(h.agent, &h.id), h.title.clone()?)))
        .into_iter()
        .collect()
}

fn report_view(v: &V<'_>, layer: &mut Vec<El>) -> Vec<El> {
    let rescanning = v.s.stats.report().is_some_and(|x| x.loading);
    let retry_para = format!(
        "A retry is a failed call followed within {} calls by a similar call of the same tool that succeeded: a round trip a rule or an env fix would have saved. Retry time is the call-to-call gap; context re-read is what the retrying model call carried. Neither is added to the other.",
        peektrace_js::num::to_js_string(crate::model::stats::RETRY_WINDOW)
    );
    let t = v.t;
    vec![
        header::header(v, rescanning, layer),
        section(
            "stats-findings-section",
            "Flagged findings",
            "cheapest fix first, then by each table's own impact",
            vec![findings::findings(v, layer)],
            t,
        ),
        section(
            "stats-causes-section",
            "Avoidable retries",
            "sessions with a retry, then sessions, then retries",
            vec![muted(retry_para, t), causes::table(v, layer)],
            t,
        ),
        section(
            "stats-fails-section",
            "What fails",
            "sessions spanned, then calls",
            vec![
                muted(
                    "A row says a call printed a failure — never that something broke. Flagged counts the calls that set the agent's own error flag; the gap between it and Calls is what an error panel alone would miss.",
                    t,
                ),
                fails_time::fails(v, layer),
            ],
            t,
        ),
        section(
            "stats-time-section",
            "What costs time",
            "seconds charged",
            vec![
                muted(
                    "Each command line is charged to exactly one job; Co-present counts the lines that hold the job and were charged to a higher-priority one. Tools that wait on a human are out of the denominator.",
                    t,
                ),
                fails_time::time(v, layer),
            ],
            t,
        ),
        section(
            "stats-waiting-section",
            "Waiting",
            "seconds parked",
            vec![evidence::waiting(&v.r.waiting, t)],
            t,
        ),
        section(
            "stats-clusters-section",
            "Error clusters",
            "rows, then sessions spanned",
            vec![evidence::clusters(v.s, &v.r.clusters, t)],
            t,
        ),
    ]
}

fn body(s: &Ui, bp: Bp, t: &Theme, layer: &mut Vec<El>) -> Vec<El> {
    let Some(slot) = s.stats.report() else {
        return vec![pending::pending(s, t)];
    };
    if let Some(e) = &slot.error {
        return vec![error_view(
            e,
            slot.loading,
            |s, tx| s.stats_ask(tx.clock()),
            t,
        )];
    }
    let Some(screen) = &slot.value else {
        return vec![pending::pending(s, t)];
    };
    let titles = titles_of(s);
    let v = V {
        s,
        r: &screen.report,
        text: screen.text.as_ref(),
        generated: &screen.generated,
        bp,
        titles: &titles,
        t,
    };
    let out = report_view(&v, layer);
    drill::sheet(&v, layer);
    out
}

/// The Stats route: one scroll column.
pub fn view(s: &Ui, cx: &mut Cx<Ui>, t: &Theme, layer: &mut Vec<El>) -> El {
    let w = cx.layout_size("stats-col").map_or(1136.0, |[w, _]| w);
    col()
        .key("stats-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col().key("stats-route").w_full().padding(24.0).child(
                col()
                    .key("stats-col")
                    .w_full()
                    .gap(32.0)
                    .pb(24.0)
                    .children(body(s, Bp::of(w), t, layer)),
            ),
        )
}
