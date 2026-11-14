//! The window: nav rail, header and the routed content.

pub mod blocks;
pub mod capabilities;
pub mod history;
pub mod session_list;
pub mod settings;
pub mod shortcuts;
pub mod stats;
pub mod summary;
pub mod widgets;

use std::time::Duration;

use rune::{Cx, El, ScrollSpec, Tx, col, el, row, spacer, text};

use self::widgets::{dot, notice};
use crate::state::{Route, Ui};
use crate::theme::{EMERALD_500, Theme, hex_or_grey, theme, ty};

const NAV_W: f32 = 256.0;
const NAV_COLLAPSED_W: f32 = 56.0;
/// The empty band over the nav that keeps the macOS traffic lights clear
/// of the brand, as the inspector's desktop shell does.
const TITLEBAR_BAND_H: f32 = 32.0;
const HEADER_H: f32 = 44.0;
const DETAIL_MAX_W: f32 = 1800.0;

/// The text a section shows until it is ported.
pub fn stub_text(section: &str) -> String {
    format!(
        "{section} is not in the desktop app yet. Use peektrace serve (web inspector) or peektrace tui."
    )
}

fn nav_row(s: &Ui, route: Route, t: &Theme) -> El {
    let active = s.route == route;
    let label = route.title();
    let body = if s.nav_collapsed {
        text(&label[..1]).font(ty::SM_MEDIUM).color(t.fg)
    } else {
        text(label).font(ty::SM_MEDIUM).color(t.fg)
    };
    t.kit
        .interactive(
            row()
                .w_full()
                .h(32.0)
                .px(10.0)
                .radius(8.0)
                .center_y()
                .fill(if active {
                    t.sidebar_accent
                } else {
                    rune::Color::TRANSPARENT
                })
                .child(body),
        )
        .key(format!("nav-{}", label.to_lowercase()))
        .role("link")
        .label(label)
        .focusable(true)
        .on_press_tx(move |s: &mut Ui, tx: &mut Tx| {
            if route == Route::Stats {
                s.enter_stats(tx.clock());
            } else {
                s.go(route);
            }
            tx.wake_after(Duration::ZERO);
            tx.set_title(if route == Route::Sessions {
                s.title()
            } else {
                format!("{} · Peektrace", route.title())
            });
        })
}

/// The nav's width: collapsed, it still clears the traffic lights.
pub fn nav_width(collapsed: bool, inset: f32) -> f32 {
    match (collapsed, inset > 0.0) {
        (false, _) => NAV_W,
        (true, false) => NAV_COLLAPSED_W,
        (true, true) => NAV_COLLAPSED_W.max(inset + 13.0),
    }
}

fn nav(s: &Ui, inset: f32, t: &Theme) -> El {
    let brand = if s.nav_collapsed {
        col().child(text("P").font(ty::BASE_SEMI).color(t.primary))
    } else {
        col()
            .gap(2.0)
            .child(text("Peektrace").font(ty::SM_MEDIUM).color(t.fg))
            .child(
                text("Agent session inspector")
                    .font(ty::XS)
                    .color(t.muted_fg),
            )
    };
    let drag_band = (inset > 0.0).then(|| {
        rune::el()
            .key("titlebar-band")
            .w_full()
            .h(TITLEBAR_BAND_H)
            .window_drag()
    });
    col()
        .key("nav")
        .w(nav_width(s.nav_collapsed, inset))
        .h_full()
        .fill(t.sidebar)
        .padding(10.0)
        .gap(4.0)
        .children(drag_band)
        .child(brand.pb(12.0).px(6.0))
        .children(
            [
                Route::Sessions,
                Route::Stats,
                Route::Memory,
                Route::Settings,
            ]
            .into_iter()
            .map(|r| nav_row(s, r, t)),
        )
        .child(spacer())
        .child(shortcuts::footer(s, t))
}

fn header(s: &Ui, t: &Theme) -> El {
    let mut r = row()
        .key("header")
        .w_full()
        .h(HEADER_H)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .window_drag()
        .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
        .child(text(s.route.title()).font(ty::SM_MEDIUM).color(t.fg));
    let hint = match s.route {
        Route::Sessions => "Context debug",
        Route::Stats => "What fails, what costs time",
        Route::Settings => "Agent roots",
        Route::Capabilities => "Support matrix",
        _ => "",
    };
    if !hint.is_empty() {
        r = r.child(text(hint).font(ty::XS).color(t.muted_fg));
    }
    r = r.child(spacer());
    if s.syncing > 0 {
        r = r.child(
            row()
                .key("syncing")
                .gap(6.0)
                .center_y()
                .child(dot(6.0, hex_or_grey(EMERALD_500)))
                .child(text("syncing").font(ty::XS).color(t.muted_fg)),
        );
    }
    if s.read_only() {
        r = r.child(widgets::badge("Read-only", t.muted_fg, t).key("read-only-badge"));
    }
    r
}

fn detail(s: &Ui, cx: &mut Cx<Ui>, t: &Theme) -> El {
    let Some(sel) = &s.selected else {
        return notice(
            "No session selected",
            "Pick a session from the list to inspect its context-budget forensics.",
            t,
        );
    };
    let mut c = col().w_full().max_w(DETAIL_MAX_W).gap(16.0).pb(48.0);
    let Some(d) = s.detail.as_ref().filter(|d| d.id == sel.id) else {
        let body = s.detail_error.as_ref().map_or_else(
            || notice("Loading session…", "", t),
            |e| {
                notice("Could not load this session", &e.to_string(), t).child(
                    rune_kit::Button::new("Retry")
                        .on_press(|s: &mut Ui| s.request_analysis())
                        .build(&t.kit)
                        .key("retry"),
                )
            },
        );
        return c.child(body);
    };
    c = c.child(summary::jump_to_history(t));
    if let Some(p) = &sel.parent {
        c = c.child(summary::parent_note(p, t));
    }
    if let Some(e) = &s.detail_error {
        c = c.child(
            text(e.to_string())
                .font(ty::XS)
                .color(t.destructive)
                .key("detail-error"),
        );
    }
    // The column is capped, `detail-col` is not; size the cards from the capped width.
    let inner = cx
        .layout_size("detail-col")
        .map_or(1000.0, |[w, _]| w.min(DETAIL_MAX_W))
        - 34.0;
    c = c
        .child(summary::verdict(d, inner, t).key(summary::SUMMARY_KEY))
        .child(crate::chart::view::growth_card(
            s,
            &d.a,
            d.growth.as_ref(),
            d.rev,
            inner,
            t,
        ))
        .child(history::heading(d, t))
        .child(history::rows(d, t));
    c
}

fn sessions(s: &Ui, cx: &mut Cx<Ui>, t: &Theme, layer: &mut Vec<El>) -> El {
    row()
        .key("sessions")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(24.0)
        .padding(16.0)
        .child(session_list::rail(s, t, layer))
        .child(
            col()
                .key("detail-scroll")
                .grow(1.0)
                .h_full()
                .min_w(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    col()
                        .key("detail-col")
                        .w_full()
                        .items_center()
                        .child(detail(s, cx, t)),
                ),
        )
}

pub fn view(s: &Ui, cx: &mut Cx<Ui>) -> El {
    s.system_dark.set(cx.system_dark());
    let t = theme(s.is_dark());
    let mut layer = Vec::new();
    let content = match s.route {
        Route::Sessions => sessions(s, cx, &t, &mut layer),
        Route::Stats => stats::view(s, cx, &t, &mut layer),
        Route::Settings => settings::page(s, &t),
        Route::Capabilities => capabilities::page(s, &t, &mut layer),
        other => el().key("stub").w_full().grow(1.0).child(notice(
            other.title(),
            &stub_text(other.title()),
            &t,
        )),
    };
    let inset = cx.titlebar_inset();
    let base = row()
        .w_full()
        .h_full()
        .fill(t.bg)
        .child(nav(s, inset, &t))
        .child(
            col()
                .grow(1.0)
                .h_full()
                .min_w(0.0)
                .child(header(s, &t))
                .child(content),
        );
    shortcuts::help(s, &t, &mut layer);
    layer.extend(shortcuts::toast(s, &t));
    rune_kit::root(base, layer)
}
