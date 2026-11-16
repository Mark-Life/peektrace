//! The growth chart on Rune: one memoised canvas for the static picture,
//! absolute text for labels, and a small hover layer on top, so a pointer
//! move rebuilds one line and one label.

use peektrace_protocol::view::{AnalyzedSession, BudgetSlices};
use rune::{
    Align, Color, El, LineCap, PointerEvent, Shape, TextStyle, Tx, canvas, col, el, memo, row, text,
};

use super::geom::{self, Growth, H, L, PLOT_H, PLOT_W, Pt, T, W, band_runs, dash, hatch_pieces};
use super::hit::snap_index_at_x;
use crate::model::format::fmt_k;
use crate::state::Ui;
use crate::theme::{RED_400, Theme, hex_or_grey, ty};

pub const PLOT_KEY: &str = "growth-plot";
const BOX_KEY: &str = "growth-box";
const MIN_W: f32 = 600.0;
const LABEL_BOX: f32 = 160.0;
const HATCH_PERIOD: f64 = 6.0;

#[expect(clippy::cast_possible_truncation, reason = "viewBox units fit f32")]
fn p(pt: Pt) -> [f32; 2] {
    [pt[0] as f32, pt[1] as f32]
}

#[expect(clippy::cast_possible_truncation, reason = "viewBox units fit f32")]
const fn f(v: f64) -> f32 {
    v as f32
}

fn red(a: f32) -> Color {
    hex_or_grey("#f85149").alpha(a)
}

fn dashed(shape: Shape, a: Pt, b: Pt, on: f64, off: f64, width: f32, c: Color) -> Shape {
    dash(a, b, on, off).into_iter().fold(shape, |s, (x, y)| {
        s.line(p(x), p(y), width, LineCap::Butt, c)
    })
}

/// `color` with `over` painted on it at `alpha`, opaque.
fn mix_over(color: Color, over: Color, alpha: f32) -> Color {
    color.mix(over, alpha)
}

fn circle(cx: f64, cy: f64, r: f64) -> Vec<[f32; 2]> {
    (0..12)
        .map(|i| {
            let a = f64::from(i) / 12.0 * std::f64::consts::TAU;
            p([cx + r * a.cos(), cy + r * a.sin()])
        })
        .collect()
}

/// The static picture in viewBox units.
pub fn picture(g: &Growth, fg: Color) -> Shape {
    let mut s = Shape::new().view_box(f(W), f(H));
    let right = L + PLOT_W;
    s = s.fill(
        vec![p([L, T]), p([right, T]), p([right, g.dz_y]), p([L, g.dz_y])],
        red(0.10),
    );
    s = dashed(s, [L, g.dz_y], [right, g.dz_y], 4.0, 3.0, 1.0, red(0.7));
    if let Some(y) = g.ceiling_y {
        s = dashed(
            s,
            [L, y],
            [right, y],
            2.0,
            4.0,
            1.0,
            Color::WHITE.alpha(0.25),
        );
    }
    for (y, _) in &g.grid {
        s = s.line(
            p([L, *y]),
            p([right, *y]),
            1.0,
            LineCap::Butt,
            Color::WHITE.alpha(0.07),
        );
    }
    for band in &g.bands {
        let color = hex_or_grey(&band.color);
        let fill = color.alpha(if band.hatch { 0.85 } else { 0.82 });
        for poly in band_runs(&band.top, &band.bottom) {
            s = s.fill(poly.into_iter().map(p).collect::<Vec<_>>(), fill);
        }
    }
    for band in g.bands.iter().filter(|b| b.hatch) {
        let ink = mix_over(hex_or_grey(&band.color), Color::BLACK, 0.45);
        for (a, b) in hatch_pieces(&band.top, &band.bottom, HATCH_PERIOD) {
            s = s.line(p(a), p(b), 2.0, LineCap::Butt, ink);
        }
    }
    if g.silhouette.len() > 1 {
        s = s.stroke(
            g.silhouette.iter().copied().map(p).collect::<Vec<_>>(),
            1.5,
            Color::WHITE.alpha(0.85),
        );
    }
    for x in &g.cliffs {
        s = dashed(
            s,
            [*x, T],
            [*x, T + PLOT_H],
            3.0,
            2.0,
            1.0,
            hex_or_grey("#d29922").alpha(0.9),
        );
    }
    for (x, y, r) in &g.dots {
        s = s.fill(circle(*x, *y, *r), Color::WHITE.alpha(0.9));
    }
    if let Some(c) = &g.cross {
        s = s.line(
            p([c.x, T]),
            p([c.x, T + PLOT_H]),
            1.0,
            LineCap::Butt,
            red(0.9),
        );
        s = s.fill(circle(c.x, c.y, 4.0), hex_or_grey("#f85149"));
    }
    let (px, py) = (g.peak.x, g.peak.y);
    s.fill(
        vec![
            p([px, py - 6.0]),
            p([px + 6.0, py]),
            p([px, py + 6.0]),
            p([px - 6.0, py]),
        ],
        fg,
    )
}

#[derive(Clone, Copy)]
enum Anchor {
    Start,
    Middle,
    End,
}

/// Text placed with its baseline at viewBox `(x, y)`.
fn label(
    s: impl Into<String>,
    x: f64,
    y: f64,
    scale: f32,
    anchor: Anchor,
    style: TextStyle,
    color: Color,
) -> El {
    let (x, top) = (f(x) * scale, f(y) * scale - style.size);
    let (left, align) = match anchor {
        Anchor::Start => (x, Align::Start),
        Anchor::Middle => (x - LABEL_BOX / 2.0, Align::Center),
        Anchor::End => (x - LABEL_BOX, Align::End),
    };
    text(s.into())
        .font(style)
        .color(color)
        .align(align)
        .w(LABEL_BOX)
        .single_line()
        .absolute(left, top)
}

fn labels(g: &Growth, scale: f32, t: &Theme) -> Vec<El> {
    let mut out = vec![label(
        &g.dz_label,
        W - geom::R,
        g.dz_y - 6.0,
        scale,
        Anchor::End,
        ty::CHART_11,
        hex_or_grey(RED_400),
    )];
    if let Some(y) = g.ceiling_y {
        out.push(label(
            "200K-model ceiling",
            L + 4.0,
            y - 5.0,
            scale,
            Anchor::Start,
            ty::CHART_10,
            t.muted_fg,
        ));
    }
    out.extend(g.grid.iter().map(|(y, s)| {
        label(
            s,
            L - 8.0,
            y + 4.0,
            scale,
            Anchor::End,
            ty::CHART_10,
            t.muted_fg,
        )
    }));
    out.extend(g.ticks.iter().map(|(x, s)| {
        label(
            s,
            *x,
            T + PLOT_H + 18.0,
            scale,
            Anchor::Middle,
            ty::CHART_10,
            t.muted_fg,
        )
    }));
    if let Some(c) = &g.cross {
        out.push(label(
            &c.label,
            c.x + 5.0,
            T + PLOT_H - 6.0,
            scale,
            Anchor::Start,
            ty::CHART_10,
            hex_or_grey(RED_400),
        ));
    }
    out.push(label(
        &g.peak.label,
        g.peak.x,
        g.peak.y - 10.0,
        scale,
        Anchor::Middle,
        ty::CHART_11,
        t.fg,
    ));
    out
}

/// A keyboard-reachable hit box over a marker that jumps to its turn.
fn marker(key: &str, m: &geom::Marker, scale: f32) -> El {
    let turn = m.turn;
    el().key(key.to_owned())
        .absolute(f(m.x) * scale - 8.0, f(m.y) * scale - 8.0)
        .size(16.0, 16.0)
        .radius(8.0)
        .role("button")
        .label(m.aria.clone())
        .focusable(true)
        .on_press_tx(move |s: &mut Ui, tx: &mut Tx| jump_to_turn(s, turn, tx))
}

/// Scroll the history to the first row of turn `n`.
pub fn jump_to_turn(s: &mut Ui, n: usize, tx: &mut Tx) {
    if let Some(key) = s.turn_row(n) {
        tx.scroll_into_view(key.to_owned());
    }
}

fn hover_index(x: f32, scale: f32, n: usize) -> Option<usize> {
    let vx = f64::from(x / scale);
    let frac = (vx - L) / PLOT_W;
    if !(0.0..=1.0).contains(&frac) {
        return None;
    }
    snap_index_at_x(L, n, PLOT_W, L + frac * PLOT_W)
}

fn hover_layer(
    a: &AnalyzedSession,
    g: &Growth,
    hover: Option<usize>,
    scale: f32,
    t: &Theme,
) -> Vec<El> {
    let n = a.snapshots.len();
    let area = el()
        .key("growth-hit")
        .absolute(0.0, f(T) * scale)
        .size(f(W) * scale, f(PLOT_H) * scale)
        .on_pointer_move_local(move |s: &mut Ui, ev: PointerEvent, _tx: &mut Tx| {
            let i = hover_index(ev.x, scale, n);
            if s.hover != i {
                s.hover = i;
            }
        })
        .on_hover_change(|s: &mut Ui, inside: bool, _tx: &mut Tx| {
            if !inside {
                s.hover = None;
            }
        })
        .on_press_tx(|s: &mut Ui, tx: &mut Tx| {
            let turn = s
                .hover
                .and_then(|i| s.detail.as_ref().and_then(|d| d.a.snapshots.get(i)))
                .map(|sn| sn.turn_index + 1);
            if let Some(turn) = turn {
                jump_to_turn(s, turn, tx);
            }
        });
    let mut out = vec![area];
    if let Some(snap) = hover.and_then(|i| a.snapshots.get(i).map(|sn| (i, sn))) {
        let (i, sn) = snap;
        let hx = g.scale.x(i);
        let right_half = hx > L + PLOT_W / 2.0;
        let line = canvas(Shape::new().line(
            [0.5, 0.0],
            [0.5, f(PLOT_H) * scale],
            1.0,
            LineCap::Butt,
            t.fg.alpha(0.5),
        ))
        .size(1.0, f(PLOT_H) * scale)
        .absolute(f(hx) * scale, f(T) * scale)
        .key("growth-guide");
        let text_x = if right_half { hx - 6.0 } else { hx + 6.0 };
        let anchor = if right_half {
            Anchor::End
        } else {
            Anchor::Start
        };
        let caption = format!("turn {} · ctx {}", sn.turn_index + 1, fmt_k(sn.ctx));
        out.push(line);
        out.push(
            label(caption, text_x, T + 12.0, scale, anchor, ty::CHART_11, t.fg).key("growth-hover"),
        );
    }
    out
}

fn legend(a: &AnalyzedSession, t: &Theme) -> El {
    let chips = BudgetSlices::STACK_ORDER
        .iter()
        .filter_map(|k| a.budget.iter().find(|b| b.key == *k))
        .map(|b| {
            row()
                .gap(6.0)
                .center_y()
                .child(
                    el().size(10.0, 10.0)
                        .radius(2.0)
                        .fill(hex_or_grey(&b.color)),
                )
                .child(text(b.label.clone()).font(ty::XS).color(t.fg))
        });
    row().w_full().gap(12.0).flex_wrap().children(chips)
}

pub fn growth_card(
    s: &Ui,
    a: &AnalyzedSession,
    g: Option<&Growth>,
    rev: u64,
    width: f32,
    t: &Theme,
) -> El {
    let title = text("Context growth timeline")
        .font(ty::BASE_SEMI)
        .color(t.fg);
    let mut card = col()
        .key("growth")
        .w_full()
        .radius(12.0)
        .border(1.0, t.border)
        .padding(16.0)
        .gap(8.0)
        .child(title);
    let Some(g) = g else {
        return card.child(
            text("No turns with usage metadata to plot.")
                .font(ty::SM)
                .color(t.muted_fg),
        );
    };
    let w = width.max(MIN_W);
    let scale = w / f(W);
    let h = f(H) * scale;
    let fg = t.fg;
    let dark = t.dark;
    let picture = memo("growth-static", (rev, w.round().to_bits(), dark), || {
        canvas(picture(g, fg)).size(w, h)
    });
    let mut plot = el()
        .key(PLOT_KEY)
        .size(w, h)
        .role("group")
        .label(g.aria.clone())
        .child(picture.absolute(0.0, 0.0))
        .children(labels(g, scale, t))
        .children(hover_layer(a, g, s.hover, scale, t));
    if let Some(c) = &g.cross {
        plot = plot.child(marker("growth-cross", c, scale));
    }
    plot = plot.child(marker("growth-peak", &g.peak, scale));
    card = card
        .child(el().key(BOX_KEY).w_full().scroll_x().child(plot))
        .child(legend(a, t));
    card
}
