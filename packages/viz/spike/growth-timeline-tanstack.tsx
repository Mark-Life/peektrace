"use client";

/** SPIKE — TanStack Charts port of `growth-timeline.tsx`.
 *
 * Evidence for the "should we adopt TanStack Charts" spike. It renders the same
 * forensic as the hand-rolled SVG original — stacked areas floor→top by
 * `STACK_ORDER` over a 0→window y-axis, with the dumb-zone band + boundary, the
 * 200K ghost ceiling, compaction cliffs, the peak marker, the first dumb-zone
 * crossing and per-turn dots — expressed as a grammar-of-graphics definition
 * instead of hand-computed path strings.
 *
 * Nothing in the product imports this. `growth-timeline.tsx` is still the
 * shipping component; this exists so the write-up in
 * `docs/spikes/tanstack-charts.md` has something real to point at.
 *
 * Two divergences from the original are deliberate and documented in the
 * write-up rather than worked around here:
 *
 * - The hatch overlay on inferred slices (`system_tools`, `unattributed`) needs
 *   an SVG `<pattern>`. TanStack Charts declares gradients as chart resources
 *   but has no pattern equivalent, so the pattern is emitted by this component
 *   into a sibling `<svg>` and referenced across documents as `url(#…)`.
 * - Text that the original anchors to the plot edge in viewBox units is anchored
 *   to data coordinates here, because the grammar has no "corner of the plot"
 *   position.
 */

import {
  areaY,
  defineChart,
  dot,
  lineY,
  rect,
  ruleX,
  ruleY,
  stack,
  text,
} from "@tanstack/charts";
import { scaleLinear } from "@tanstack/charts-scales/linear";
import { Chart } from "@tanstack/react-charts";
import type {
  AnalyzedSession,
  BudgetKey,
} from "@workspace/core/services/sessions/schema";
import { cn } from "@workspace/ui/lib/utils";
import { useId, useMemo } from "react";
import {
  CAT_META,
  fmtK,
  PERCENT,
  STACK_ORDER,
} from "../src/lib/session-format";

/** Slices core infers rather than measures; drawn with a hatch overlay. */
const HATCHED = new Set<BudgetKey>(["system_tools", "unattributed"]);
/** Ghost ceiling line drawn for 200K-window models when the window is larger. */
const CEIL_200K = 200_000;
/** Rendered chart height in CSS px, and the SSR/pre-measure width. */
const HEIGHT = 440;
const INITIAL_WIDTH = 1000;
/** Fill opacity for the stacked areas, matching the original. */
const AREA_OPACITY = 0.82;
/** Per-turn dot sizing: radius caps at `maxR`, growing with output tokens. */
const DOT = { maxR: 4, baseR: 1, tokensPerPx: 6000 } as const;
/** Peak marker radius, in px. */
const PEAK_R = 5;
/** Label offsets in px, applied to `text` marks after scaling. */
const OFF = { label: -8, crossLabel: 14, peakLabel: -12 } as const;
/** Font sizes matching the original's 10/11px axis and annotation text. */
const FONT = { annotation: 11, small: 10 } as const;
/** Y-axis gridlines, evenly spaced as fractions of the context window. */
const GRID_INTERVALS = 4;

/** One stacked-area row: a single category's tokens at a single turn. */
interface SliceRow {
  readonly key: BudgetKey;
  readonly tokens: number;
  readonly turn: number;
}

/** One point on the true-total silhouette. */
interface TotalRow {
  readonly ctx: number;
  readonly outputTokens: number;
  readonly turn: number;
}

/** An annotation anchored to a data coordinate. */
interface Label {
  readonly label: string;
  readonly x: number;
  readonly y: number;
}

export interface GrowthTimelineTanstackProps {
  readonly a: AnalyzedSession;
  /** Optional cap on rendered width, in CSS px. Matches the original's prop. */
  readonly maxWidth?: number;
}

/** The TanStack Charts stacked-area context-growth timeline. */
export const GrowthTimelineTanstack = ({
  a,
  maxWidth,
}: GrowthTimelineTanstackProps) => {
  // The chart scopes its own resource IDs; this app-owned pattern must do the
  // same or two timelines on one page would collide.
  const hatchId = useId();
  const definition = useMemo(() => {
    const snaps = a.snapshots;
    const n = snaps.length;
    const win = a.contextWindow;
    const dz = a.dumbZoneFraction * win;
    // Turns are 1-based on the axis, matching the original's x tick labels.
    const lastTurn = Math.max(1, n);

    const slices: SliceRow[] = snaps.flatMap((s, i) =>
      STACK_ORDER.map((key) => ({
        turn: i + 1,
        key,
        tokens: s.slices[key] ?? 0,
      }))
    );
    const totals: TotalRow[] = snaps.map((s, i) => ({
      turn: i + 1,
      ctx: s.ctx,
      outputTokens: s.outputTokens,
    }));
    const hatched = slices.filter((r) => HATCHED.has(r.key));

    const dumbZoneLabel: Label[] = [
      {
        x: lastTurn,
        y: dz,
        label: `DUMB ZONE · ${Math.round(a.dumbZoneFraction * PERCENT)}% = ${fmtK(dz)}`,
      },
    ];
    const ceilingLabel: Label[] =
      win > CEIL_200K
        ? [{ x: 1, y: CEIL_200K, label: "200K-model ceiling" }]
        : [];
    const peak: Label[] = [
      {
        x: a.peakTurnIndex + 1,
        y: a.peakContextTokens,
        label: `peak ${fmtK(a.peakContextTokens)}`,
      },
    ];
    const cross: Label[] =
      a.dumbZoneCrossTurn >= 0
        ? [
            {
              x: a.dumbZoneCrossTurn + 1,
              y: snaps[a.dumbZoneCrossTurn]?.ctx ?? 0,
              label: `entered @ turn ${a.dumbZoneCrossTurn + 1}`,
            },
          ]
        : [];

    return defineChart({
      marks: [
        // dumb-zone danger band
        rect([{ x1: 1, x2: lastTurn, y1: dz, y2: win }], {
          x1: "x1",
          x2: "x2",
          y1: "y1",
          y2: "y2",
          fill: "rgb(248 81 73)",
          fillOpacity: 0.1,
          inset: 0,
        }),
        // stacked areas, floor → top by STACK_ORDER
        areaY(slices, {
          x: "turn",
          y: "tokens",
          z: "key",
          fill: (r: SliceRow) => CAT_META[r.key].color,
          fillOpacity: AREA_OPACITY,
          layout: stack({ order: [...STACK_ORDER] }),
        }),
        // hatch overlay for inferred slices, stacked identically so the
        // geometry lines up with the flat fills underneath
        areaY(hatched, {
          x: "turn",
          y: "tokens",
          z: "key",
          fill: `url(#${hatchId})`,
          fillOpacity: 1,
          layout: stack({ order: [...STACK_ORDER] }),
        }),
        // dumb-zone boundary
        ruleY([dz], {
          stroke: "rgb(248 81 73)",
          strokeOpacity: 0.7,
          strokeDasharray: "4 3",
        }),
        text(dumbZoneLabel, {
          x: "x",
          y: "y",
          text: "label",
          anchor: "end",
          dy: OFF.label,
          fill: "rgb(248 113 113)",
          fontSize: FONT.annotation,
        }),
        // 200K ghost ceiling
        ...(ceilingLabel.length > 0
          ? [
              ruleY([CEIL_200K], {
                stroke: "currentColor",
                strokeOpacity: 0.25,
                strokeDasharray: "2 4",
              }),
              text(ceilingLabel, {
                x: "x",
                y: "y",
                text: "label",
                anchor: "start",
                dy: OFF.label,
                fillOpacity: 0.7,
                fontSize: FONT.small,
              }),
            ]
          : []),
        // true-total silhouette
        lineY(totals, {
          x: "turn",
          y: "ctx",
          stroke: "currentColor",
          strokeOpacity: 0.85,
          strokeWidth: 1.5,
        }),
        // compaction cliffs
        ruleX(
          a.compactionTurns.map((t) => t + 1),
          {
            stroke: "rgb(210 153 34)",
            strokeOpacity: 0.9,
            strokeDasharray: "3 2",
          }
        ),
        // dumb-zone first crossing
        ...(cross.length > 0
          ? [
              ruleX([a.dumbZoneCrossTurn + 1], {
                stroke: "rgb(248 81 73)",
                strokeOpacity: 0.9,
              }),
              dot(cross, { x: "x", y: "y", r: 4, fill: "#f85149" }),
              text(cross, {
                x: "x",
                y: "y",
                text: "label",
                anchor: "start",
                dy: OFF.crossLabel,
                fill: "rgb(248 113 113)",
                fontSize: FONT.small,
              }),
            ]
          : []),
        // peak marker
        dot(peak, { x: "x", y: "y", r: PEAK_R, fill: "currentColor" }),
        text(peak, {
          x: "x",
          y: "y",
          text: "label",
          dy: OFF.peakLabel,
          fill: "currentColor",
          fontSize: FONT.annotation,
        }),
        // per-turn dots, radius growing with output tokens
        dot(totals, {
          x: "turn",
          y: "ctx",
          r: (t: TotalRow) =>
            Math.min(DOT.maxR, DOT.baseR + t.outputTokens / DOT.tokensPerPx),
          fill: "currentColor",
          fillOpacity: 0.9,
        }),
      ],
      x: {
        scale: scaleLinear().domain([1, lastTurn]),
        axis: { ticks: { format: (v: number) => String(Math.round(v)) } },
      },
      y: {
        scale: scaleLinear().domain([0, win]),
        grid: true,
        axis: { ticks: { count: GRID_INTERVALS, format: fmtK } },
      },
      clip: true,
    });
  }, [a, hatchId]);

  if (a.snapshots.length === 0) {
    return (
      <p className="text-muted-foreground text-sm">
        No turns with usage metadata to plot.
      </p>
    );
  }

  return (
    <section
      className="flex flex-col gap-2 rounded-lg border border-border p-4"
      data-testid="growth-timeline-tanstack"
    >
      <div>
        <h2 className="font-semibold text-base">Context growth timeline</h2>
        <p className="text-muted-foreground text-sm">
          Real context per turn, attributed to categories. The red band is the
          dumb zone (&gt;{Math.round(a.dumbZoneFraction * PERCENT)}% of window).
        </p>
      </div>

      {/* TanStack Charts has no pattern resource; the hatch fill referenced by
          the overlay area lives in this sibling defs-only SVG. */}
      <svg aria-hidden="true" className="absolute size-0" focusable="false">
        <title>Hatch pattern definitions</title>
        <defs>
          <pattern
            height="6"
            id={hatchId}
            patternTransform="rotate(45)"
            patternUnits="userSpaceOnUse"
            width="6"
          >
            <line
              stroke="rgba(0,0,0,0.45)"
              strokeWidth="2"
              x1="0"
              x2="0"
              y1="0"
              y2="6"
            />
          </pattern>
        </defs>
      </svg>

      <Chart
        ariaDescription={`Stacked context by category across ${a.snapshots.length} turns, against a ${fmtK(a.contextWindow)} context window.`}
        ariaLabel="Context growth over turns"
        className={cn(maxWidth !== undefined && "mx-auto")}
        definition={definition}
        height={HEIGHT}
        initialWidth={INITIAL_WIDTH}
        style={maxWidth === undefined ? undefined : { maxWidth }}
      />

      <div className="flex flex-wrap gap-3 text-xs">
        {STACK_ORDER.filter((k) => a.budget.some((b) => b.key === k)).map(
          (k) => (
            <span className="inline-flex items-center gap-1.5" key={k}>
              <span
                className="inline-block size-2.5 rounded-sm"
                style={{ background: CAT_META[k].color }}
              />
              {CAT_META[k].label}
            </span>
          )
        )}
      </div>
    </section>
  );
};
