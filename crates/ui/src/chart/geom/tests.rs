use peektrace_protocol::view::{BudgetSlices, TurnSnapshot};

use super::*;
use crate::model::fixtures::analysis;

fn snap(turn_index: usize, ctx: f64, tool_results: f64) -> TurnSnapshot {
    TurnSnapshot {
        turn_index,
        model: "m".into(),
        ctx,
        output_tokens: 3000.0,
        cache_read_tokens: 0.0,
        slices: BudgetSlices {
            system_tools: ctx - tool_results,
            tool_results,
            ..BudgetSlices::default()
        },
        ts: None,
    }
}

#[test]
fn maps_x_and_y() {
    let one = Scale { n: 1, win: 1000.0 };
    assert!((one.x(0) - (L + PLOT_W / 2.0)).abs() < 1e-9);
    let s = Scale { n: 3, win: 1000.0 };
    assert!((s.x(0) - L).abs() < 1e-9);
    assert!((s.x(2) - (L + PLOT_W)).abs() < 1e-9);
    assert!((s.y(0.0) - (T + PLOT_H)).abs() < 1e-9);
    assert!((s.y(1000.0) - T).abs() < 1e-9);
    assert!((s.y(5000.0) - T).abs() < 1e-9);
    assert!((s.y(-5.0) - (T + PLOT_H)).abs() < 1e-9);
}

#[test]
fn ticks_space_out() {
    let ticks = |n| tick_indexes(n).collect::<Vec<_>>();
    assert_eq!(ticks(1), [0]);
    assert_eq!(ticks(9), (0..9).collect::<Vec<_>>());
    assert_eq!(ticks(10), (0..10).collect::<Vec<_>>());
    assert_eq!(ticks(11), [0, 2, 4, 6, 8, 10]);
    assert_eq!(ticks(552).len(), 10);
    assert_eq!(ticks(552)[1], 56);
}

#[test]
fn dots_cap() {
    assert!((dot_radius(0.0) - 1.0).abs() < 1e-9);
    assert!((dot_radius(6000.0) - 2.0).abs() < 1e-9);
    assert!((dot_radius(1e9) - 4.0).abs() < 1e-9);
}

#[test]
fn pos_prefers_the_turns_own_snapshot() {
    let snaps = vec![
        snap(0, 1.0, 0.0),
        snap(1, 1.0, 0.0),
        snap(0, 1.0, 0.0),
        snap(2, 1.0, 0.0),
    ];
    assert_eq!(pos(&snaps, 2), 3);
    assert_eq!(pos(&snaps, 1), 1);
    assert_eq!(pos(&snaps, 9), 3);
}

#[test]
fn growth_builds_every_layer() {
    let mut a = analysis();
    a.context_window = 1_000_000.0;
    a.snapshots = (0..5)
        .map(|i| {
            snap(
                i,
                f64::from(u32::try_from(i).unwrap_or(0)) * 100_000.0,
                50_000.0,
            )
        })
        .collect();
    a.peak_context_tokens = 400_000.0;
    a.peak_turn_index = 4;
    a.dumb_zone_cross_turn = 4;
    a.compaction_turns = vec![2];
    a.turn_count = 5;
    let g = growth(&a).unwrap_or_else(|| panic!("no chart"));
    assert_eq!(g.bands.len(), 10);
    assert_eq!(
        g.grid.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
        ["0", "250K", "500K", "750K", "1M"]
    );
    assert_eq!(g.dz_label, "DUMB ZONE · 40% = 400K");
    assert!(g.ceiling_y.is_some());
    assert_eq!(g.cliffs.len(), 1);
    assert_eq!(g.peak.label, "peak 400K");
    assert_eq!(
        g.cross.as_ref().map(|c| c.label.as_str()),
        Some("entered @ turn 5")
    );
    assert_eq!(g.aria, "Context growth over 5 turns, peak 400K at turn 5");
    a.context_window = 0.0;
    assert!(growth(&a).is_none());
    a.context_window = 1000.0;
    a.snapshots.clear();
    assert!(growth(&a).is_none());
}

fn is_simple(poly: &[Pt]) -> bool {
    let n = poly.len();
    let cross = |o: Pt, a: Pt, b: Pt| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let proper = |a: Pt, b: Pt, c: Pt, d: Pt| {
        let (d1, d2) = (cross(c, d, a), cross(c, d, b));
        let (d3, d4) = (cross(a, b, c), cross(a, b, d));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    };
    let mut seen = std::collections::HashSet::new();
    if !poly
        .iter()
        .all(|p| seen.insert((p[0].to_bits(), p[1].to_bits())))
    {
        return false;
    }
    (0..n).all(|i| {
        (0..n).all(|j| {
            let adjacent = i == j || (i + 1) % n == j || (j + 1) % n == i;
            adjacent || !proper(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n])
        })
    })
}

#[test]
fn runs_split_at_pinches() {
    let xs = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0];
    let thickness = [0.0, 5.0, 0.0, 0.0, 4.0, 3.0];
    let bottom: Vec<Pt> = xs.iter().map(|&x| [x, 100.0]).collect();
    let top: Vec<Pt> = xs
        .iter()
        .zip(thickness)
        .map(|(&x, t)| [x, 100.0 - t])
        .collect();
    let runs = band_runs(&top, &bottom);
    assert_eq!(runs.len(), 2);
    assert_eq!(
        runs[0],
        vec![[0.0, 100.0], [10.0, 95.0], [20.0, 100.0], [10.0, 100.0]]
    );
    assert!(runs.iter().all(|r| is_simple(r)));
    let flat: Vec<Pt> = xs.iter().map(|&x| [x, 100.0]).collect();
    assert_eq!(band_runs(&flat, &flat), Vec::<Vec<Pt>>::new());
    let pinch = [3.0, 0.0, 3.0];
    let top: Vec<Pt> = xs[..3]
        .iter()
        .zip(pinch)
        .map(|(&x, t)| [x, 100.0 - t])
        .collect();
    let runs = band_runs(&top, &bottom[..3]);
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|r| is_simple(r)));
}

#[test]
fn hatch_stays_inside_each_segment() {
    let top = [[0.0, 50.0], [40.0, 30.0], [80.0, 60.0]];
    let bottom = [[0.0, 100.0], [40.0, 100.0], [80.0, 100.0]];
    let pieces = hatch_pieces(&top, &bottom, 6.0);
    assert_ne!(pieces.len(), 0);
    let inside = |p: Pt| {
        let seg = usize::from(p[0] > 40.0);
        let (a, b) = (top[seg], top[seg + 1]);
        let t = (p[0] - a[0]) / (b[0] - a[0]);
        let y_top = a[1] + (b[1] - a[1]) * t;
        p[1] >= y_top - 1e-6 && p[1] <= 100.0 + 1e-6 && (-1e-6..=80.0 + 1e-6).contains(&p[0])
    };
    let bad: Vec<_> = pieces
        .iter()
        .filter(|(a, b)| !(inside(*a) && inside(*b)))
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
    let slope = |(a, b): &(Pt, Pt)| (b[1] - a[1]) / (b[0] - a[0]);
    assert!(pieces.iter().all(|p| (slope(p) + 1.0).abs() < 1e-6));
}

#[test]
fn dashes_split_a_segment() {
    let d = dash([0.0, 0.0], [10.0, 0.0], 4.0, 3.0);
    assert_eq!(d, vec![([0.0, 0.0], [4.0, 0.0]), ([7.0, 0.0], [10.0, 0.0])]);
    assert_eq!(dash([1.0, 1.0], [1.0, 1.0], 4.0, 3.0), vec![]);
}
