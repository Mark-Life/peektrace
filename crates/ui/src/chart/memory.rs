//! Memory chart geometry: the link graph ring, the type donut and the
//! index budget bars, in the inspector's own units (`link-graph.tsx`,
//! `type-donut.tsx` with `@tanstack/charts` `pie`, `budget-gauge.tsx`).

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use peektrace_js::num::js_round;
use peektrace_protocol::view::memory::{BudgetView, GraphView, TypeCounts};

use crate::model::format::fmt;

/// The ring's viewBox side, centre and radius.
pub const SIZE: f64 = 320.0;
pub const CENTER: f64 = SIZE / 2.0;
pub const RADIUS: f64 = 120.0;
const NODE_MIN_R: f64 = 5.0;
const NODE_R_RANGE: f64 = 9.0;
const LABEL_OFFSET: f64 = 2.0;
/// The widest the graph draws, in points.
pub const MAX_WIDTH: f32 = 360.0;

/// The donut's box side, inner radius ratio and seam between slices.
pub const DONUT: f64 = 140.0;
const INNER_RATIO: f64 = 0.57;
const GAP_ANGLE: f64 = 0.03;
/// Arcs are drawn as polygons split this finely, as `polarSectorBoundary` does.
const ARC_STEP: f64 = PI / 24.0;

#[derive(Debug, Clone, PartialEq)]
pub struct RingNode {
    pub slug: String,
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub in_index: bool,
    /// The label's baseline centre.
    pub label: [f64; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct RingEdge {
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub resolved: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ring {
    pub nodes: Vec<RingNode>,
    pub edges: Vec<RingEdge>,
}

/// Nodes evenly on a ring from 12 o'clock, sized by bytes; an edge draws
/// only when both ends are nodes (index pointers never are). A repeated
/// slug takes its last place.
pub fn ring(g: &GraphView) -> Ring {
    #[expect(clippy::cast_precision_loss, reason = "node counts are small")]
    let n = g.nodes.len().max(1) as f64;
    let pos: HashMap<&str, [f64; 2]> = g
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            #[expect(clippy::cast_precision_loss, reason = "node counts are small")]
            let angle = (i as f64 / n) * PI * 2.0 - PI / 2.0;
            (
                node.slug.as_str(),
                [CENTER + RADIUS * angle.cos(), CENTER + RADIUS * angle.sin()],
            )
        })
        .collect();
    #[expect(clippy::cast_precision_loss, reason = "file sizes fit f64")]
    let max_bytes = g
        .nodes
        .iter()
        .map(|n| n.bytes as f64)
        .fold(1.0_f64, f64::max);
    let edges = g
        .edges
        .iter()
        .filter_map(|e| {
            let from = pos.get(e.from.as_str())?;
            let to = pos.get(e.resolved_to.as_deref().unwrap_or(&e.to))?;
            Some(RingEdge {
                from: *from,
                to: *to,
                resolved: e.resolved,
            })
        })
        .collect();
    let nodes = g
        .nodes
        .iter()
        .filter_map(|node| {
            let [x, y] = *pos.get(node.slug.as_str())?;
            #[expect(clippy::cast_precision_loss, reason = "file sizes fit f64")]
            let r = NODE_MIN_R + (node.bytes as f64 / max_bytes) * NODE_R_RANGE;
            Some(RingNode {
                slug: node.slug.clone(),
                x,
                y,
                r,
                in_index: node.in_index,
                label: [x, y + (-r - LABEL_OFFSET)],
            })
        })
        .collect();
    Ring { nodes, edges }
}

/// One donut slice: d3 angles (0 at 12 o'clock, clockwise).
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub kind: String,
    pub count: u64,
    /// `--chart-N`, 1-based.
    pub chart: usize,
    pub start: f64,
    pub end: f64,
}

/// The `--chart-N` token of a memory type.
pub fn type_chart(kind: &str) -> usize {
    match kind {
        "user" => 1,
        "feedback" => 2,
        "project" => 3,
        "reference" => 4,
        _ => 5,
    }
}

/// `pie(data, {value: "count", gapAngle: 0.03})` over a full turn: each
/// positive count in document order, a seam after every slice.
pub fn donut(counts: &TypeCounts) -> Vec<Slice> {
    let data: Vec<(&str, u64)> = counts
        .0
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| (k.as_str(), *n))
        .collect();
    #[expect(clippy::cast_precision_loss, reason = "counts are small")]
    let total: f64 = data.iter().map(|(_, n)| *n as f64).sum();
    #[expect(clippy::cast_precision_loss, reason = "counts are small")]
    let drawable = TAU - data.len() as f64 * GAP_ANGLE;
    let mut cursor = 0.0;
    let last = data.len().saturating_sub(1);
    data.iter()
        .enumerate()
        .map(|(i, (kind, count))| {
            #[expect(clippy::cast_precision_loss, reason = "counts are small")]
            let fraction = if total == 0.0 {
                0.0
            } else {
                *count as f64 / total
            };
            let start = cursor;
            let end = if i == last {
                TAU - GAP_ANGLE
            } else {
                cursor + drawable * fraction
            };
            cursor = end + GAP_ANGLE;
            Slice {
                kind: (*kind).to_owned(),
                count: *count,
                chart: type_chart(kind),
                start,
                end,
            }
        })
        .collect()
}

/// The donut's outer and inner radius in a `size` box.
pub fn donut_radii(size: f64) -> (f64, f64) {
    let r = size / 2.0;
    (r, r * INNER_RATIO)
}

/// A point on a d3 arc around `(cx, cy)`. Plain products, no fused
/// multiply-add, so the numbers round as in JS.
pub fn arc_point(cx: f64, cy: f64, r: f64, a: f64) -> [f64; 2] {
    [cx + r * a.sin(), cy - r * a.cos()]
}

/// The slice outline: outer arc forward, inner arc back.
pub fn slice_polygon(s: &Slice, cx: f64, cy: f64, outer: f64, inner: f64) -> Vec<[f64; 2]> {
    let sweep = s.end - s.start;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few dozen steps"
    )]
    let steps = (sweep.abs() / ARC_STEP).ceil().max(1.0) as usize;
    let at = |k: usize| {
        #[expect(clippy::cast_precision_loss, reason = "small step counts")]
        let f = k as f64 / steps as f64;
        s.start + sweep * f
    };
    let outer_arc = (0..=steps).map(|k| arc_point(cx, cy, outer, at(k)));
    let inner_arc = (0..=steps).rev().map(|k| arc_point(cx, cy, inner, at(k)));
    outer_arc.chain(inner_arc).collect()
}

/// A bar's fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarTone {
    Ok,
    Warn,
    Over,
}

impl BarTone {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "emerald500",
            Self::Warn => "amber500",
            Self::Over => "red500",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub label: &'static str,
    /// `77 / 25,600 B`.
    pub text: String,
    /// Filled percent, 0 to 100.
    pub pct: f64,
    pub over: bool,
    pub tone: BarTone,
}

fn bar(label: &'static str, value: u64, max: u64, unit: &str) -> Bar {
    #[expect(clippy::cast_precision_loss, reason = "index sizes fit f64")]
    let (v, m) = (value as f64, max as f64);
    let pct = js_round(v / m * 100.0).min(100.0);
    let over = value > max;
    let tone = if over {
        BarTone::Over
    } else if pct > 80.0 {
        BarTone::Warn
    } else {
        BarTone::Ok
    };
    Bar {
        label,
        text: format!("{} / {} {unit}", fmt(v), fmt(m)),
        pct,
        over,
        tone,
    }
}

/// The two index bars; none for a vault with no MEMORY.md.
pub fn budget_bars(b: &BudgetView) -> Vec<Bar> {
    if b.kind == "absent" {
        return Vec::new();
    }
    vec![
        bar("Index lines", b.lines, b.max_lines, "lines"),
        bar("Index bytes", b.bytes, b.max_bytes, "B"),
    ]
}

#[cfg(test)]
mod tests;
