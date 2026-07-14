/** Context-growth timeline — a stacked-area chart of real per-turn context.
 *
 * Areas stack floor → top by `STACK_ORDER` over a 0→window y-axis, with: the
 * dumb-zone danger band + boundary line, the 200K-model ghost ceiling,
 * compaction cliffs, the peak marker, and the first dumb-zone crossing marker.
 */
import type {
  AnalyzedSession,
  TurnSnapshot,
} from "@workspace/core/services/sessions/schema";
import { cn } from "@workspace/ui/lib/utils";
import { CAT_META, fmtK, PERCENT, STACK_ORDER } from "../lib/session-format";

const GEOM = { W: 1000, H: 440, L: 76, R: 18, T: 22, B: 46 } as const;
const HATCHED = new Set(["system_tools", "unattributed"]);
/** Ghost ceiling line drawn for 200K-window models when the window is larger. */
const CEIL_200K = 200_000;
/** Y-axis gridlines, evenly spaced as fractions of the context window. */
const GRID_INTERVALS = 4;
const GRID_FRACTIONS = Array.from(
  { length: GRID_INTERVALS + 1 },
  (_, i) => i / GRID_INTERVALS
);
/** Fill opacity for flat vs hatched stacked areas. */
const AREA_OPACITY = { flat: 0.82, hatch: 0.85 } as const;
/** Per-turn dot sizing: radius caps at `maxR`, growing with output tokens. */
const DOT = { maxR: 4, baseR: 1, tokensPerPx: 6000 } as const;
/** Small SVG label/marker offsets, in viewBox px. */
const OFF = {
  label: 6,
  ceilLabel: 5,
  axisLabelX: 8,
  axisLabelY: 4,
  cross: 5,
  peak: 6,
  peakLabel: 10,
  xTick: 18,
} as const;
/** Cap the number of x-axis ticks so they never crowd. */
const MAX_X_TICKS = 10;

/**
 * Width the timeline geometry is drawn for, in CSS px. Rendering the SVG at
 * this width maps 1 viewBox unit to 1 px, so labels and hairlines land at their
 * designed size; pass it as `maxWidth` in layouts wider than the chart.
 */
export const GROWTH_TIMELINE_WIDTH = GEOM.W;

/** Smallest fraction of native scale at which the axis labels stay readable. */
const MIN_LEGIBLE_SCALE = 0.9;

/**
 * Smallest width, in CSS px, at which the 10–11px axis labels stay readable.
 * Pass it as `minWidth` in containers that can get narrower than the chart; the
 * SVG then scrolls sideways instead of shrinking below legibility.
 */
export const GROWTH_TIMELINE_MIN_WIDTH = Math.round(GEOM.W * MIN_LEGIBLE_SCALE);

/** Build the floor→top stacked-area path strings for one category index. */
const buildAreas = ({
  snaps,
  x,
  y,
}: {
  readonly snaps: readonly TurnSnapshot[];
  readonly x: (i: number) => number;
  readonly y: (tok: number) => number;
}) => {
  const n = snaps.length;
  let running = new Array<number>(n).fill(0);
  const areas: { key: string; d: string; color: string; hatch: boolean }[] = [];
  for (const key of STACK_ORDER) {
    const lower = running.slice();
    const upper = running.map((lo, i) => lo + (snaps[i]?.slices[key] ?? 0));
    const top = upper.map((v, i) => `${x(i).toFixed(1)} ${y(v).toFixed(1)}`);
    let d = `M ${top.join(" L ")}`;
    for (let i = n - 1; i >= 0; i--) {
      d += ` L ${x(i).toFixed(1)} ${y(lower[i] ?? 0).toFixed(1)}`;
    }
    areas.push({
      key,
      d: `${d} Z`,
      color: CAT_META[key].color,
      hatch: HATCHED.has(key),
    });
    running = upper;
  }
  return areas;
};

interface GrowthTimelineProps {
  readonly a: AnalyzedSession;
  /**
   * Optional cap on the rendered SVG width, in CSS px. Left unset the SVG fills
   * its container, which scales the 10–11px labels and hairline strokes up past
   * the size the geometry was drawn for. Set it (see `GROWTH_TIMELINE_WIDTH`) in
   * containers wider than the chart to keep the drawing at its native scale.
   */
  readonly maxWidth?: number;
  /**
   * Optional floor on the rendered SVG width, in CSS px. Left unset the SVG
   * shrinks with its container, which scales the 10–11px labels below legibility
   * on narrow viewports. Set it (see `GROWTH_TIMELINE_MIN_WIDTH`) to keep the
   * drawing readable and let the chart scroll sideways instead.
   */
  readonly minWidth?: number;
}

/** The inline-SVG stacked-area context-growth timeline. */
export const GrowthTimeline = ({
  a,
  maxWidth,
  minWidth,
}: GrowthTimelineProps) => {
  const { W, H, L, R, T, B } = GEOM;
  const plotW = W - L - R;
  const plotH = H - T - B;
  const snaps = a.snapshots;
  const n = snaps.length;
  const win = a.contextWindow;

  if (n === 0) {
    return (
      <p className="text-muted-foreground text-sm">
        No turns with usage metadata to plot.
      </p>
    );
  }

  const x = (i: number) => (n <= 1 ? L + plotW / 2 : L + (i / (n - 1)) * plotW);
  const y = (tok: number) =>
    T + plotH - (Math.min(Math.max(tok, 0), win) / win) * plotH;

  const areas = buildAreas({ snaps, x, y });
  const silhouette = `M ${snaps
    .map((s, i) => `${x(i).toFixed(1)} ${y(s.ctx).toFixed(1)}`)
    .join(" L ")}`;
  const dzY = y(a.dumbZoneFraction * win);
  const grid = GRID_FRACTIONS;
  const peakX = x(a.peakTurnIndex);
  const peakY = y(a.peakContextTokens);
  const crossX = a.dumbZoneCrossTurn >= 0 ? x(a.dumbZoneCrossTurn) : 0;

  return (
    <section
      className="flex flex-col gap-2 rounded-lg border border-border p-4"
      data-testid="growth-timeline"
    >
      <div>
        <h2 className="font-semibold text-base">Context growth timeline</h2>
        <p className="text-muted-foreground text-sm">
          Real context per turn, attributed to categories. The red band is the
          dumb zone (&gt;{Math.round(a.dumbZoneFraction * PERCENT)}% of window).
        </p>
      </div>
      <div className="overflow-x-auto">
        <svg
          aria-label="Context growth over turns"
          className={cn("w-full", maxWidth !== undefined && "mx-auto")}
          data-testid="timeline-svg"
          preserveAspectRatio="xMidYMid meet"
          role="img"
          style={{
            maxWidth: maxWidth === undefined ? undefined : `${maxWidth}px`,
            minWidth: minWidth === undefined ? undefined : `${minWidth}px`,
          }}
          viewBox={`0 0 ${W} ${H}`}
        >
          <defs>
            <pattern
              height="6"
              id="tl-hatch"
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

          {/* dumb-zone danger band + boundary */}
          <rect
            data-testid="timeline-dumbzone"
            fill="rgba(248,81,73,0.10)"
            height={(dzY - T).toFixed(1)}
            width={plotW}
            x={L}
            y={T}
          />
          <line
            stroke="rgba(248,81,73,0.7)"
            strokeDasharray="4 3"
            x1={L}
            x2={W - R}
            y1={dzY.toFixed(1)}
            y2={dzY.toFixed(1)}
          />
          <text
            className="fill-red-400"
            fontSize="11"
            textAnchor="end"
            x={W - R}
            y={(dzY - OFF.label).toFixed(1)}
          >
            DUMB ZONE · {Math.round(a.dumbZoneFraction * PERCENT)}% ={" "}
            {fmtK(a.dumbZoneFraction * win)}
          </text>

          {/* 200K ghost ceiling */}
          {win > CEIL_200K ? (
            <>
              <line
                stroke="rgba(255,255,255,0.25)"
                strokeDasharray="2 4"
                x1={L}
                x2={W - R}
                y1={y(CEIL_200K).toFixed(1)}
                y2={y(CEIL_200K).toFixed(1)}
              />
              <text
                className="fill-muted-foreground"
                fontSize="10"
                x={L + OFF.axisLabelY}
                y={(y(CEIL_200K) - OFF.ceilLabel).toFixed(1)}
              >
                200K-model ceiling
              </text>
            </>
          ) : null}

          {/* gridlines + y labels */}
          {grid.map((f) => {
            const yy = y(f * win);
            return (
              <g key={f}>
                <line
                  stroke="rgba(255,255,255,0.07)"
                  x1={L}
                  x2={W - R}
                  y1={yy.toFixed(1)}
                  y2={yy.toFixed(1)}
                />
                <text
                  className="fill-muted-foreground"
                  fontSize="10"
                  textAnchor="end"
                  x={L - OFF.axisLabelX}
                  y={(yy + OFF.axisLabelY).toFixed(1)}
                >
                  {fmtK(f * win)}
                </text>
              </g>
            );
          })}

          {/* stacked areas */}
          {areas.map((ar) => (
            <path
              d={ar.d}
              fill={ar.color}
              fillOpacity={ar.hatch ? AREA_OPACITY.hatch : AREA_OPACITY.flat}
              key={ar.key}
            >
              <title>{CAT_META[ar.key as keyof typeof CAT_META].label}</title>
            </path>
          ))}
          {areas
            .filter((ar) => ar.hatch)
            .map((ar) => (
              <path d={ar.d} fill="url(#tl-hatch)" key={`h-${ar.key}`} />
            ))}

          {/* true-total silhouette */}
          <path
            d={silhouette}
            fill="none"
            stroke="rgba(255,255,255,0.85)"
            strokeWidth="1.5"
          />

          {/* compaction cliffs */}
          {a.compactionTurns.map((ti) => (
            <line
              key={`c-${ti}`}
              stroke="rgba(210,153,34,0.9)"
              strokeDasharray="3 2"
              x1={x(ti).toFixed(1)}
              x2={x(ti).toFixed(1)}
              y1={T}
              y2={T + plotH}
            />
          ))}

          {/* dumb-zone first crossing */}
          {a.dumbZoneCrossTurn >= 0 ? (
            <g data-testid="timeline-cross">
              <line
                stroke="rgba(248,81,73,0.9)"
                x1={crossX.toFixed(1)}
                x2={crossX.toFixed(1)}
                y1={T}
                y2={T + plotH}
              />
              <circle
                cx={crossX.toFixed(1)}
                cy={y(snaps[a.dumbZoneCrossTurn]?.ctx ?? 0).toFixed(1)}
                fill="#f85149"
                r="4"
              />
              <text
                className="fill-red-400"
                fontSize="10"
                x={(crossX + OFF.cross).toFixed(1)}
                y={(T + plotH - OFF.label).toFixed(1)}
              >
                entered @ turn {a.dumbZoneCrossTurn + 1}
              </text>
            </g>
          ) : null}

          {/* peak marker */}
          <g data-testid="timeline-peak">
            <path
              className="fill-foreground"
              d={`M ${peakX.toFixed(1)} ${(peakY - OFF.peak).toFixed(1)} L ${(peakX + OFF.peak).toFixed(1)} ${peakY.toFixed(1)} L ${peakX.toFixed(1)} ${(peakY + OFF.peak).toFixed(1)} L ${(peakX - OFF.peak).toFixed(1)} ${peakY.toFixed(1)} Z`}
            />
            <text
              className="fill-foreground"
              fontSize="11"
              textAnchor="middle"
              x={peakX.toFixed(1)}
              y={(peakY - OFF.peakLabel).toFixed(1)}
            >
              peak {fmtK(a.peakContextTokens)}
            </text>
          </g>

          {/* per-turn dots */}
          {snaps.map((s, i) => (
            <circle
              cx={x(i).toFixed(1)}
              cy={y(s.ctx).toFixed(1)}
              fill="rgba(255,255,255,0.9)"
              key={s.turnIndex}
              r={Math.min(
                DOT.maxR,
                DOT.baseR + s.outputTokens / DOT.tokensPerPx
              ).toFixed(1)}
            />
          ))}

          {/* x ticks */}
          {snaps.map((s, i) =>
            i % Math.max(1, Math.ceil(n / MAX_X_TICKS)) === 0 ? (
              <text
                className="fill-muted-foreground"
                fontSize="10"
                key={`t-${s.turnIndex}`}
                textAnchor="middle"
                x={x(i).toFixed(1)}
                y={(T + plotH + OFF.xTick).toFixed(1)}
              >
                {i + 1}
              </text>
            ) : null
          )}
        </svg>
      </div>

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
