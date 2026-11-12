//! Growth chart geometry in the web chart's viewBox units (1000 x 440), so
//! it can be compared with the SVG point for point. The view scales it.

use peektrace_protocol::view::{AnalyzedSession, BudgetSlices, TurnSnapshot};

use crate::model::format::fmt_k;
use crate::model::jsnum::{js_round, to_js_string};

pub const W: f64 = 1000.0;
pub const H: f64 = 440.0;
pub const L: f64 = 76.0;
pub const R: f64 = 18.0;
pub const T: f64 = 22.0;
pub const B: f64 = 46.0;
pub const PLOT_W: f64 = W - L - R;
pub const PLOT_H: f64 = H - T - B;
/// Ghost ceiling for 200K-window models when the window is larger.
const CEIL_200K: f64 = 200_000.0;
const GRID: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
const MAX_X_TICKS: usize = 10;
const HATCHED: [&str; 2] = ["system_tools", "unattributed"];
/// Thinner than this (viewBox units) a band draws nothing.
const MIN_THICKNESS: f64 = 0.01;

pub type Pt = [f64; 2];

/// Fallback colours per stack key, as core's `CAT_META` has them.
const DEFAULT_COLORS: [&str; 10] = [
    "#6e7681", "#58a6ff", "#bc8cff", "#39c5cf", "#3fb950", "#f0883e", "#d29922", "#db61a2",
    "#8a929e", "#484f58",
];

/// Maps snapshot positions and token counts to viewBox points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub n: usize,
    pub win: f64,
}

impl Scale {
    #[expect(clippy::cast_precision_loss, reason = "snapshot counts are small")]
    pub fn x(self, i: usize) -> f64 {
        if self.n <= 1 {
            L + PLOT_W / 2.0
        } else {
            L + i as f64 / (self.n - 1) as f64 * PLOT_W
        }
    }

    pub fn y(self, tok: f64) -> f64 {
        T + PLOT_H - tok.max(0.0).min(self.win) / self.win * PLOT_H
    }
}

/// One stacked band: its top and bottom outline per snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub key: &'static str,
    pub color: String,
    pub hatch: bool,
    pub top: Vec<Pt>,
    pub bottom: Vec<Pt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// 1-based turn the marker jumps to.
    pub turn: usize,
    pub x: f64,
    pub y: f64,
    pub label: String,
    pub aria: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Growth {
    pub scale: Scale,
    pub bands: Vec<Band>,
    pub silhouette: Vec<Pt>,
    pub dz_y: f64,
    pub dz_label: String,
    pub ceiling_y: Option<f64>,
    pub grid: Vec<(f64, String)>,
    pub cliffs: Vec<f64>,
    /// `(x, y, r)` per snapshot.
    pub dots: Vec<(f64, f64, f64)>,
    pub ticks: Vec<(f64, String)>,
    pub cross: Option<Marker>,
    pub peak: Marker,
    pub aria: String,
}

/// The position of turn `t` among the snapshots: the first snapshot of that
/// turn, else `t` itself clamped to the list. Interleaved turns give extra
/// snapshots, so the turn index alone can point past its snapshot.
pub fn pos(snaps: &[TurnSnapshot], t: usize) -> usize {
    snaps
        .iter()
        .position(|s| s.turn_index == t)
        .unwrap_or_else(|| t.min(snaps.len().saturating_sub(1)))
}

/// Dot radius: grows with output tokens, capped at 4.
pub fn dot_radius(output_tokens: f64) -> f64 {
    (1.0 + output_tokens / 6000.0).min(4.0)
}

/// Snapshot positions that get an x tick: every `ceil(n/10)`-th.
pub fn tick_indexes(n: usize) -> impl Iterator<Item = usize> {
    (0..n).step_by(n.div_ceil(MAX_X_TICKS).max(1))
}

fn color_of(a: &AnalyzedSession, k: usize, key: &str) -> String {
    a.budget
        .iter()
        .find(|b| b.key == key)
        .map_or_else(|| DEFAULT_COLORS[k].to_owned(), |b| b.color.clone())
}

fn bands(a: &AnalyzedSession, s: Scale) -> Vec<Band> {
    let slices: Vec<[f64; 10]> = a.snapshots.iter().map(|sn| sn.slices.stacked()).collect();
    let mut running = vec![0.0; a.snapshots.len()];
    BudgetSlices::STACK_ORDER
        .iter()
        .enumerate()
        .map(|(k, &key)| {
            let lower = running.clone();
            running
                .iter_mut()
                .zip(&slices)
                .for_each(|(r, sl)| *r += sl[k]);
            let pts = |v: &[f64]| {
                v.iter()
                    .enumerate()
                    .map(|(i, &t)| [s.x(i), s.y(t)])
                    .collect()
            };
            Band {
                key,
                color: color_of(a, k, key),
                hatch: HATCHED.contains(&key),
                top: pts(&running),
                bottom: pts(&lower),
            }
        })
        .collect()
}

/// The chart, or `None` when there is nothing to plot (no snapshots, or no
/// window to scale against).
pub fn growth(a: &AnalyzedSession) -> Option<Growth> {
    let n = a.snapshots.len();
    let win = a.context_window;
    if n == 0 || win.is_nan() || win <= 0.0 {
        return None;
    }
    let s = Scale { n, win };
    let snaps = &a.snapshots;
    let dz = a.dumb_zone_fraction;
    let cross = usize::try_from(a.dumb_zone_cross_turn).ok().map(|c| {
        let p = pos(snaps, c);
        let turn = c + 1;
        Marker {
            turn,
            x: s.x(p),
            y: s.y(snaps.get(p).map_or(0.0, |sn| sn.ctx)),
            label: format!("entered @ turn {turn}"),
            aria: format!("Entered dumb zone at turn {turn}: jump to it in history"),
        }
    });
    let peak_turn = a.peak_turn_index + 1;
    let peak_k = fmt_k(a.peak_context_tokens);
    Some(Growth {
        scale: s,
        bands: bands(a, s),
        silhouette: snaps
            .iter()
            .enumerate()
            .map(|(i, sn)| [s.x(i), s.y(sn.ctx)])
            .collect(),
        dz_y: s.y(dz * win),
        dz_label: format!(
            "DUMB ZONE · {}% = {}",
            to_js_string(js_round(dz * 100.0)),
            fmt_k(dz * win)
        ),
        ceiling_y: (win > CEIL_200K).then(|| s.y(CEIL_200K)),
        grid: GRID
            .iter()
            .map(|f| (s.y(f * win), fmt_k(f * win)))
            .collect(),
        cliffs: a
            .compaction_turns
            .iter()
            .map(|&t| s.x(pos(snaps, t)))
            .collect(),
        dots: snaps
            .iter()
            .enumerate()
            .map(|(i, sn)| (s.x(i), s.y(sn.ctx), dot_radius(sn.output_tokens)))
            .collect(),
        ticks: tick_indexes(n)
            .map(|i| (s.x(i), (i + 1).to_string()))
            .collect(),
        cross,
        peak: Marker {
            turn: peak_turn,
            x: s.x(pos(snaps, a.peak_turn_index)),
            y: s.y(a.peak_context_tokens),
            label: format!("peak {peak_k}"),
            aria: format!("Peak {peak_k} at turn {peak_turn}: jump to it in history"),
        },
        aria: format!(
            "Context growth over {} turns, peak {peak_k} at turn {peak_turn}",
            a.turn_count
        ),
    })
}

fn thick(top: &[Pt], bottom: &[Pt], i: usize) -> bool {
    (bottom[i][1] - top[i][1]) > MIN_THICKNESS
}

/// The band as simple polygons: one per stretch of snapshots where it has
/// thickness, split at every point where it pinches to nothing, so no
/// polygon touches itself. Top points run forward, bottom points back.
pub fn band_runs(top: &[Pt], bottom: &[Pt]) -> Vec<Vec<Pt>> {
    let n = top.len().min(bottom.len());
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..n.saturating_sub(1) {
        let area = thick(top, bottom, i) || thick(top, bottom, i + 1);
        match start {
            Some(a) if !area => {
                runs.push(outline(top, bottom, a, i));
                start = None;
            }
            Some(a) if !thick(top, bottom, i) => {
                runs.push(outline(top, bottom, a, i));
                start = Some(i);
            }
            None if area => start = Some(i),
            _ => {}
        }
    }
    if let Some(a) = start {
        runs.push(outline(top, bottom, a, n - 1));
    }
    runs.retain(|r| r.len() >= 3);
    runs
}

fn outline(top: &[Pt], bottom: &[Pt], a: usize, b: usize) -> Vec<Pt> {
    let mut pts: Vec<Pt> = top[a..=b].to_vec();
    pts.extend(bottom[a..=b].iter().rev());
    pts.dedup();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

/// Clip the infinite line `p + t*d` to the convex polygon `poly`
/// (Cyrus-Beck); `None` when it misses.
fn clip_line(p: Pt, d: Pt, poly: &[Pt]) -> Option<(Pt, Pt)> {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let n = poly.len();
    // Orientation of the polygon, so edge normals point inward.
    let area: f64 = (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let normal = [-(b[1] - a[1]) * sign, (b[0] - a[0]) * sign];
        let num = normal[0] * (p[0] - a[0]) + normal[1] * (p[1] - a[1]);
        let den = normal[0] * d[0] + normal[1] * d[1];
        if den.abs() < 1e-12 {
            if num < 0.0 {
                return None;
            }
        } else if den > 0.0 {
            lo = lo.max(-num / den);
        } else {
            hi = hi.min(-num / den);
        }
    }
    (lo < hi && lo.is_finite() && hi.is_finite()).then(|| {
        (
            [p[0] + lo * d[0], p[1] + lo * d[1]],
            [p[0] + hi * d[0], p[1] + hi * d[1]],
        )
    })
}

/// 45° hatch lines `period` apart, clipped to each segment of the band.
pub fn hatch_pieces(top: &[Pt], bottom: &[Pt], period: f64) -> Vec<(Pt, Pt)> {
    let n = top.len().min(bottom.len());
    // Lines run along (-1, 1); their offsets step along (1, 1) / sqrt 2.
    let d = [-1.0, 1.0];
    let mut out = Vec::new();
    for i in 0..n.saturating_sub(1) {
        if !thick(top, bottom, i) && !thick(top, bottom, i + 1) {
            continue;
        }
        let mut quad = vec![top[i], top[i + 1], bottom[i + 1], bottom[i]];
        quad.dedup();
        if quad.len() < 3 {
            continue;
        }
        let sums = quad
            .iter()
            .map(|p| (p[0] + p[1]) / std::f64::consts::SQRT_2);
        let (min, max) = sums.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
        let mut k = (min / period).ceil();
        while k * period <= max {
            let c = k * period * std::f64::consts::SQRT_2;
            if let Some(seg) = clip_line([c / 2.0, c / 2.0], d, &quad) {
                out.push(seg);
            }
            k += 1.0;
        }
    }
    out
}

/// Dash pieces of the segment `a`-`b`: `on` drawn, `off` skipped.
pub fn dash(a: Pt, b: Pt, on: f64, off: f64) -> Vec<(Pt, Pt)> {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    if len <= 0.0 || on <= 0.0 {
        return Vec::new();
    }
    let at = |t: f64| {
        [
            a[0] + (b[0] - a[0]) * t / len,
            a[1] + (b[1] - a[1]) * t / len,
        ]
    };
    let mut out = Vec::new();
    let mut t = 0.0;
    while t < len {
        out.push((at(t), at((t + on).min(len))));
        t += on + off;
    }
    out
}

#[cfg(test)]
mod tests;
