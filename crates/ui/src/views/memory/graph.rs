//! The link graph: a memoised canvas of edges and nodes in the ring's
//! viewBox, absolute labels, and an invisible pressable box over each node.

use std::collections::HashMap;

use peektrace_protocol::view::memory::VaultView;
use rune::{Align, Color, El, LineCap, Shape, TextStyle, Tx, canvas, el, memo, text};

use super::hint;
use super::paint::{amber_500, emerald_500, red_500};
use super::table::navigate;
use crate::chart::geom::dash;
use crate::chart::memory::{MAX_WIDTH, Ring, SIZE, ring};
use crate::state::Ui;
use crate::theme::Theme;

const LABEL_BOX: f32 = 120.0;
const LABEL_PT: f32 = 8.0;

#[expect(clippy::cast_possible_truncation, reason = "viewBox units fit f32")]
const fn f(v: f64) -> f32 {
    v as f32
}

fn circle(cx: f64, cy: f64, r: f64) -> Vec<[f32; 2]> {
    (0..24)
        .map(|i| {
            let a = f64::from(i) / 24.0 * std::f64::consts::TAU;
            [f(cx + r * a.cos()), f(cy + r * a.sin())]
        })
        .collect()
}

fn picture(r: &Ring, muted: Color) -> Shape {
    let mut s = Shape::new().view_box(f(SIZE), f(SIZE));
    for e in &r.edges {
        if e.resolved {
            s = s.line(
                [f(e.from[0]), f(e.from[1])],
                [f(e.to[0]), f(e.to[1])],
                1.0,
                LineCap::Butt,
                muted.alpha(0.4),
            );
        } else {
            for (a, b) in dash(e.from, e.to, 4.0, 3.0) {
                s = s.line(
                    [f(a[0]), f(a[1])],
                    [f(b[0]), f(b[1])],
                    1.0,
                    LineCap::Butt,
                    red_500().alpha(0.6),
                );
            }
        }
    }
    for n in &r.nodes {
        let fill = if n.in_index {
            emerald_500()
        } else {
            amber_500()
        };
        s = s.fill(circle(n.x, n.y, n.r), fill.alpha(0.7));
    }
    s
}

/// The graph at `avail` points wide, at most [`MAX_WIDTH`].
pub fn graph(s: &Ui, v: &VaultView, avail: f32, t: &Theme) -> El {
    let key = format!("mem-graph:{}", v.slug);
    if v.graph.nodes.is_empty() {
        return hint(key, "No memory files to graph.", t);
    }
    let w = avail.clamp(120.0, MAX_WIDTH);
    let scale = w / f(SIZE);
    let r = ring(&v.graph);
    let muted = t.muted_fg;
    let shape = picture(&r, muted);
    let pic = memo(
        format!("mem-graph-static:{}", v.slug),
        (s.memory.rev, w.round().to_bits(), t.dark),
        move || canvas(shape).size(w, w),
    );
    let style = TextStyle::new(LABEL_PT * scale, 400.0);
    let labels = r.nodes.iter().map(|n| {
        let (x, y) = (f(n.label[0]) * scale, f(n.label[1]) * scale);
        text(n.slug.clone())
            .font(style)
            .color(muted)
            .align(Align::Center)
            .w(LABEL_BOX)
            .single_line()
            .absolute(x - LABEL_BOX / 2.0, y - style.size)
    });
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let nodes: Vec<El> = r
        .nodes
        .iter()
        .map(|n| {
            let k = seen.entry(n.slug.as_str()).or_insert(0);
            *k += 1;
            let node_key = if *k == 1 {
                format!("mem-node:{}:{}", v.slug, n.slug)
            } else {
                format!("mem-node:{}:{}:{k}", v.slug, n.slug)
            };
            let side = f(n.r * 2.0) * scale;
            let (vault, slug) = (v.slug.clone(), n.slug.clone());
            el().key(node_key)
                .absolute(f(n.x - n.r) * scale, f(n.y - n.r) * scale)
                .size(side, side)
                .radius(side / 2.0)
                .role("button")
                .label(format!("Open {}", n.slug))
                .focusable(true)
                .on_press_tx(move |s: &mut Ui, tx: &mut Tx| navigate(s, &vault, &slug, tx))
        })
        .collect();
    el().key(key)
        .size(w, w)
        .role("group")
        .label("Memory link graph")
        .child(pic.absolute(0.0, 0.0))
        .children(labels)
        .children(nodes)
}
