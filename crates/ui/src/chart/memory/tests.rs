use peektrace_protocol::view::memory::{GraphEdgeView, GraphNodeView};

use super::*;

fn node(slug: &str, bytes: u64) -> GraphNodeView {
    GraphNodeView {
        slug: slug.to_owned(),
        kind: "user".to_owned(),
        bytes,
        in_index: true,
    }
}

fn edge(from: &str, to: &str, resolved_to: Option<&str>) -> GraphEdgeView {
    GraphEdgeView {
        from: from.to_owned(),
        to: to.to_owned(),
        kind: "wiki".to_owned(),
        resolved: resolved_to.is_some(),
        resolved_to: resolved_to.map(str::to_owned),
        line: Some(1),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn one_node_sits_at_the_top() {
    let r = ring(&GraphView {
        nodes: vec![node("a", 0)],
        edges: vec![],
    });
    assert!(close(r.nodes[0].x, 160.0) && close(r.nodes[0].y, 40.0));
    assert!(close(r.nodes[0].r, 5.0));
}

#[test]
fn four_nodes_go_clockwise_and_edges_need_both_ends() {
    let g = GraphView {
        nodes: ["a", "b", "c", "d"]
            .iter()
            .zip([10, 20, 30, 40])
            .map(|(s, b)| node(s, b))
            .collect(),
        edges: vec![
            edge("a", "b", Some("b")),
            edge("__index__", "a", Some("a")),
            edge("c", "missing", None),
            edge("d", "a", None),
        ],
    };
    let r = ring(&g);
    let at: Vec<(f64, f64)> = r.nodes.iter().map(|n| (n.x, n.y)).collect();
    assert!(close(at[1].0, 280.0) && close(at[1].1, 160.0));
    assert!(close(at[2].0, 160.0) && close(at[2].1, 280.0));
    assert!(close(r.nodes[3].r, 14.0));
    assert!(close(r.nodes[0].label[1], 40.0 - r.nodes[0].r - 2.0));
    assert_eq!(r.edges.len(), 2);
    assert!(r.edges[0].resolved && !r.edges[1].resolved);
}

#[test]
fn one_slice_keeps_a_seam() {
    let s = donut(&TypeCounts(vec![("user".into(), 3), ("x".into(), 0)]));
    assert_eq!(s.len(), 1);
    assert!(close(s[0].start, 0.0) && close(s[0].end, TAU - 0.03));
    assert_eq!(s[0].chart, 1);
    let s = donut(&TypeCounts(vec![("42".into(), 1), ("unknown".into(), 1)]));
    let drawable = TAU - 0.06;
    assert!(close(s[0].end, drawable / 2.0));
    assert!(close(s[1].start, drawable / 2.0 + 0.03));
    assert_eq!((s[0].chart, s[1].chart), (5, 5));
    let poly = slice_polygon(&s[0], 70.0, 70.0, 70.0, 39.9);
    assert!(close(poly[0][0], 70.0) && close(poly[0][1], 0.0));
}

#[test]
fn bars_warn_past_80_and_go_red_over_the_cliff() {
    let b = |lines, bytes| BudgetView {
        lines,
        max_lines: 200,
        bytes,
        max_bytes: 25_600,
        over_budget: false,
        below_fold_count: 0,
        kind: "index".to_owned(),
    };
    let bars = budget_bars(&b(230, 77));
    assert_eq!(bars[0].text, "230 / 200 lines");
    assert_eq!((bars[0].pct, bars[0].tone), (100.0, BarTone::Over));
    assert_eq!(bars[1].text, "77 / 25,600 B");
    assert_eq!(bars[1].tone, BarTone::Ok);
    assert_eq!(budget_bars(&b(161, 0))[0].tone, BarTone::Warn);
    assert_eq!(budget_bars(&b(1, 0))[0].pct, 1.0);
    let absent = BudgetView {
        kind: "absent".into(),
        ..b(0, 0)
    };
    assert_eq!(budget_bars(&absent), vec![]);
}
