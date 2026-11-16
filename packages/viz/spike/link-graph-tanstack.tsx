"use client";

/** SPIKE — TanStack Charts port of `link-graph.tsx`.
 *
 * Evidence for the "should we adopt TanStack Charts" spike, and the more
 * interesting half of it: a node-link graph is the shape a chart-type library
 * handles badly. The shipping `link-graph.tsx` sidesteps that by placing nodes
 * on a fixed ring — cheap, deterministic, and topologically uninformative.
 *
 * This port uses `forceLayout` from `@tanstack/charts/network/force`, which
 * runs a fixed number of synchronous d3-force ticks at build time and returns
 * settled coordinates plus padded domains. That makes clusters visible while
 * staying deterministic and chart-size independent: resizing remaps the settled
 * coordinates without re-simulating.
 *
 * Nothing in the product imports this. `link-graph.tsx` is still the shipping
 * component.
 */

import { defineChart, dot, link, text } from "@tanstack/charts";
import { scaleLinear } from "@tanstack/charts-scales/linear";
import { scaleOrdinal } from "@tanstack/charts-scales/ordinal";
import { forceLayout } from "@tanstack/charts/network/force";
import { Chart } from "@tanstack/react-charts";
import type { GraphData } from "@workspace/view-core/api-types.gen";
import { useMemo } from "react";

/** Node radius scales from this minimum across this range by byte size. */
const NODE_MIN_R = 5;
const NODE_R_RANGE = 9;

/** Rendered size the graph is drawn for, in CSS px. */
const DEFAULT_MAX_WIDTH = 360;
const MAX_WIDTH_CEILING = 480;
const HEIGHT = 320;

/** Label offset above a node, in px. */
const LABEL_DY = -14;
const LABEL_FONT = 8;

/** Force-simulation parameters. Fixed iterations keep the layout deterministic
 * across renders and between server and browser. */
const FORCES = {
  collide: 12,
  iterations: 300,
  linkDistance: 46,
  manyBody: -140,
} as const;

/** A node row fed to the force layout. */
interface NodeRow {
  readonly bytes: number;
  readonly id: string;
  readonly inIndex: boolean;
}

/** An edge row fed to the force layout. */
interface EdgeRow {
  readonly resolved: boolean;
  readonly source: string;
  readonly target: string;
}

export interface LinkGraphTanstackProps {
  readonly graph: GraphData;
  /** Rendered width cap in px, clamped to the geometry's usable range. */
  readonly maxWidth?: number;
  readonly onSelect?: (slug: string) => void;
}

/** Render the force-directed link graph. Empty vaults show a hint instead. */
export const LinkGraphTanstack = ({
  graph,
  onSelect,
  maxWidth = DEFAULT_MAX_WIDTH,
}: LinkGraphTanstackProps) => {
  const definition = useMemo(() => {
    const known = new Set(graph.nodes.map((n) => n.slug));

    const nodes: NodeRow[] = graph.nodes.map((n) => ({
      bytes: n.bytes,
      id: n.slug,
      inIndex: n.inIndex,
    }));

    // Only edges whose endpoints both exist can be laid out; the shipping
    // component drops the same ones by failing its position lookup.
    const edges: EdgeRow[] = graph.edges.flatMap((e) => {
      const target = e.resolvedTo ?? e.to;

      return known.has(e.from) && known.has(target)
        ? [{ resolved: e.resolved, source: e.from, target }]
        : [];
    });

    const maxBytes = Math.max(1, ...nodes.map((n) => n.bytes));

    const settled = forceLayout(nodes, edges, {
      forces: [
        { distance: FORCES.linkDistance, type: "link" },
        { strength: FORCES.manyBody, type: "manyBody" },
        { type: "center" },
        { radius: FORCES.collide, type: "collide" },
      ],
      iterations: FORCES.iterations,
      nodeKey: "id",
      source: "source",
      target: "target",
    });

    // `link.strokeDasharray` and `dot.fill` are constants rather than visual
    // channels, so resolved/unresolved edges become two marks instead of one
    // mark with a data-driven dash, and node colour goes through the shared
    // ordinal colour scale.
    const resolvedEdges = settled.links.filter((e) => e.resolved);
    const danglingEdges = settled.links.filter((e) => !e.resolved);

    return defineChart({
      color: {
        scale: scaleOrdinal(
          ["indexed", "unindexed"],
          ["rgb(16 185 129)", "rgb(245 158 11)"]
        ),
      },
      guides: false,
      margin: 0,
      marks: [
        link(resolvedEdges, {
          key: (e) => `${e.source}->${e.target}`,
          stroke: "currentColor",
          strokeOpacity: 0.4,
          strokeWidth: 1,
          x1: "x1",
          x2: "x2",
          y1: "y1",
          y2: "y2",
        }),
        link(danglingEdges, {
          key: (e) => `${e.source}->${e.target}`,
          stroke: "rgb(239 68 68)",
          strokeDasharray: "4 3",
          strokeOpacity: 0.6,
          strokeWidth: 1,
          x1: "x1",
          x2: "x2",
          y1: "y1",
          y2: "y2",
        }),
        dot(settled.nodes, {
          color: (n) => (n.inIndex ? "indexed" : "unindexed"),
          fillOpacity: 0.7,
          key: "id",
          r: (n) => NODE_MIN_R + (n.bytes / maxBytes) * NODE_R_RANGE,
          x: "x",
          y: "y",
        }),
        text(settled.nodes, {
          dy: LABEL_DY,
          fillOpacity: 0.7,
          fontSize: LABEL_FONT,
          key: "id",
          text: "id",
          x: "x",
          y: "y",
        }),
      ],
      x: { scale: scaleLinear().domain(settled.xDomain) },
      y: { scale: scaleLinear().domain(settled.yDomain) },
    });
  }, [graph]);

  if (graph.nodes.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="graph-empty">
        No memory files to graph.
      </p>
    );
  }

  return (
    <Chart
      ariaLabel="Memory link graph"
      data-testid="link-graph-tanstack"
      definition={definition}
      height={HEIGHT}
      initialWidth={Math.min(maxWidth, MAX_WIDTH_CEILING)}
      onSelect={
        onSelect
          ? (point) => {
              // The focus datum is a union over every mark in the definition,
              // so a node click has to be narrowed out of the link rows.
              const datum = point?.datum;

              if (datum && "id" in datum) {
                onSelect(datum.id);
              }
            }
          : undefined
      }
      style={{ maxWidth: Math.min(maxWidth, MAX_WIDTH_CEILING) }}
    />
  );
};
