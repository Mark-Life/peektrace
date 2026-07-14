"use client";

/** Wikilink graph for a vault, drawn as a static SVG circular layout.
 *
 * Nodes (memory files) sit on a circle; edges are the resolved `[[wikilinks]]`
 * + index pointers between them. Node radius scales with byte size; unresolved
 * edges are dashed/red. Deterministic layout (no physics) keeps it cheap and
 * stable across renders. Clicking a node calls `onSelect(slug)`.
 */
import type { GraphData } from "@workspace/core/services/memory/types";
import { cn } from "@workspace/ui/lib/utils";

/** SVG viewbox size + circle radius for the ring layout. */
const SIZE = 320;
const RADIUS = 120;
const CENTER = SIZE / 2;
/** Node radius scales from this minimum across this range by byte size. */
const NODE_MIN_R = 5;
const NODE_R_RANGE = 9;
/** Vertical offset of a node label above its circle. */
const LABEL_OFFSET = 2;

/** Rendered width (px) the ring geometry was drawn for. */
const DEFAULT_MAX_WIDTH = 360;
/** Hard ceiling on rendered width: past this the 8px labels and 1px strokes
 * are scaled far enough past their drawn size that the graph reads as soft. */
const MAX_WIDTH_CEILING = 480;

/** Place nodes evenly on a ring; returns slug → {x,y}. */
const layout = (slugs: readonly string[]) => {
  const positions = new Map<string, { x: number; y: number }>();
  const n = Math.max(1, slugs.length);
  slugs.forEach((slug, i) => {
    const angle = (i / n) * Math.PI * 2 - Math.PI / 2;
    positions.set(slug, {
      x: CENTER + RADIUS * Math.cos(angle),
      y: CENTER + RADIUS * Math.sin(angle),
    });
  });
  return positions;
};

interface LinkGraphProps {
  readonly graph: typeof GraphData.Type;
  /** Rendered width cap in px, clamped to the geometry's usable range. */
  readonly maxWidth?: number;
  readonly onSelect?: (slug: string) => void;
}

/** Render the link graph. Empty vaults show a hint instead. */
export const LinkGraph = ({
  graph,
  onSelect,
  maxWidth = DEFAULT_MAX_WIDTH,
}: LinkGraphProps) => {
  if (graph.nodes.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="graph-empty">
        No memory files to graph.
      </p>
    );
  }
  const pos = layout(graph.nodes.map((node) => node.slug));
  const maxBytes = Math.max(1, ...graph.nodes.map((node) => node.bytes));
  return (
    <svg
      aria-label="Memory link graph"
      className="h-auto w-full"
      data-testid="link-graph"
      role="img"
      style={{ maxWidth: Math.min(maxWidth, MAX_WIDTH_CEILING) }}
      viewBox={`0 0 ${SIZE} ${SIZE}`}
    >
      <title>Memory link graph</title>
      {graph.edges.map((edge) => {
        const from = pos.get(edge.from);
        const to = pos.get(edge.resolvedTo ?? edge.to);
        if (!(from && to)) {
          return null;
        }
        return (
          <line
            className={cn(
              edge.resolved ? "stroke-muted-foreground/40" : "stroke-red-500/60"
            )}
            key={`${edge.from}->${edge.to}:${edge.line ?? 0}`}
            strokeDasharray={edge.resolved ? undefined : "4 3"}
            strokeWidth={1}
            x1={from.x}
            x2={to.x}
            y1={from.y}
            y2={to.y}
          />
        );
      })}
      {graph.nodes.map((node) => {
        const p = pos.get(node.slug);
        if (!p) {
          return null;
        }
        const r = NODE_MIN_R + (node.bytes / maxBytes) * NODE_R_RANGE;
        const select = onSelect ? () => onSelect(node.slug) : undefined;
        return (
          // biome-ignore lint/a11y/noStaticElementInteractions: SVG group exposes role=button + keyboard handler for node selection
          <g
            aria-label={onSelect ? `Open ${node.slug}` : undefined}
            className={onSelect ? "cursor-pointer" : undefined}
            key={node.slug}
            onClick={select}
            onKeyDown={
              select
                ? (e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      select();
                    }
                  }
                : undefined
            }
            role={onSelect ? "button" : undefined}
            tabIndex={onSelect ? 0 : undefined}
          >
            <circle
              className={cn(
                node.inIndex ? "fill-emerald-500/70" : "fill-amber-500/70"
              )}
              cx={p.x}
              cy={p.y}
              r={r}
            />
            <text
              className="fill-muted-foreground text-[8px]"
              dy={-r - LABEL_OFFSET}
              textAnchor="middle"
              x={p.x}
              y={p.y}
            >
              {node.slug}
            </text>
          </g>
        );
      })}
    </svg>
  );
};
