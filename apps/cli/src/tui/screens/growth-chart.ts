/** Pure builder for the terminal context-growth column chart.
 *
 * Turns an `AnalyzedSession` into a discrete grid of colored cells: one column
 * per turn (bucketed when turns exceed the available width), each column a
 * stacked bar of budget categories floor -> top over a 0..contextWindow y-axis.
 * Everything the component needs to render (grid, axis ticks, threshold row,
 * peak/compaction columns, legend, header) is precomputed here so the component
 * stays presentational and this stays unit-testable.
 */
import type {
  AnalyzedSession,
  BudgetKey,
  BudgetSlices,
} from "@workspace/core/services/sessions/schema";
import { C, fmtK, fmtPct } from "../theme";

/** Stacking order, floor -> top; mirrors the SVG timeline. */
export const STACK_ORDER: readonly BudgetKey[] = [
  "system_tools",
  "listings",
  "memory",
  "files",
  "prompts",
  "tool_results",
  "assistant_text",
  "thinking",
  "other",
  "unattributed",
];

/** A painted grid cell (category color) or an empty slot. */
export type Cell = { readonly color: string } | null;

/** A y-axis gridline: the row it lands on and its token label. */
export interface YTick {
  readonly label: string;
  readonly row: number;
}

/** An x-axis tick: the column it lands on and its 1-based turn label. */
export interface XTick {
  readonly col: number;
  readonly label: string;
}

/** A legend entry for one present (nonzero) category. */
export interface LegendEntry {
  readonly color: string;
  readonly key: BudgetKey;
  readonly label: string;
}

/** The fully-computed chart, ready to render as terminal rows. */
export interface GrowthChart {
  readonly compactionCols: readonly number[];
  /** True when there is nothing to plot (no window / no turns). */
  readonly empty: boolean;
  readonly header: string;
  readonly height: number;
  readonly legend: readonly LegendEntry[];
  /** Column holding the peak turn; `-1` when the chart is empty. */
  readonly peakCol: number;
  /** `height` rows (top first) x `width` cols; each entry a `Cell`. */
  readonly rows: Cell[][];
  readonly thresholdLabel: string;
  /** Row (from top) of the dumb-zone boundary; `< 0` when off-grid. */
  readonly thresholdRow: number;
  readonly width: number;
  readonly xTicks: readonly XTick[];
  readonly yTicks: readonly YTick[];
}

/** Fallback cell tint for a category with no budget-provided color. */
const DIM = C.textFaint;

/** Y-axis gridlines drawn, counting both the full and the empty end. */
const GRID_LINES = 5;

/** Y-axis gridline fractions of the window, top (full) -> bottom (empty). */
const GRID_FRACTIONS = Array.from(
  { length: GRID_LINES },
  (_, i) => 1 - i / (GRID_LINES - 1)
);

/** Cap on x-axis ticks so labels never crowd. */
const MAX_X_TICKS = 10;

const PERCENT = 100;

/** The empty chart shown as "no turns with usage to plot". */
const emptyChart = (): GrowthChart => ({
  rows: [],
  width: 0,
  height: 0,
  yTicks: [],
  xTicks: [],
  thresholdRow: -1,
  thresholdLabel: "",
  peakCol: -1,
  compactionCols: [],
  legend: [],
  header: "",
  empty: true,
});

/** One resolved column: the representative snapshot's ctx + slices + origin. */
interface Column {
  readonly ctx: number;
  readonly slices: BudgetSlices;
  /** 1-based turn number of the representative snapshot (for x ticks). */
  readonly turn: number;
}

/**
 * Bucket snapshots into `cols` columns. When turns fit, each is its own column;
 * otherwise a column is represented by its highest-ctx snapshot (keeps peaks
 * visible rather than averaging them away).
 */
const bucketColumns = (
  snaps: AnalyzedSession["snapshots"],
  cols: number
): Column[] => {
  const n = snaps.length;
  const out: Column[] = [];
  for (let c = 0; c < cols; c++) {
    const start = Math.floor((c * n) / cols);
    const end = Math.max(start + 1, Math.floor(((c + 1) * n) / cols));
    let rep = snaps[start];
    let repIdx = start;
    for (let i = start + 1; i < end && i < n; i++) {
      const s = snaps[i];
      if (s && (!rep || s.ctx > rep.ctx)) {
        rep = s;
        repIdx = i;
      }
    }
    if (rep) {
      out.push({ ctx: rep.ctx, slices: rep.slices, turn: repIdx + 1 });
    }
  }
  return out;
};

/** Map a snapshot array-position to its column index. */
const colOf = (pos: number, n: number, cols: number): number =>
  Math.max(0, Math.min(cols - 1, Math.floor((pos * cols) / n)));

/** Paint one column's stacked cells (bottom -> top) into `rows`. */
const paintColumn = ({
  rows,
  col,
  slices,
  win,
  height,
  colorOf,
}: {
  readonly rows: Cell[][];
  readonly col: number;
  readonly slices: BudgetSlices;
  readonly win: number;
  readonly height: number;
  readonly colorOf: ReadonlyMap<BudgetKey, string>;
}): void => {
  let filled = 0;
  for (const key of STACK_ORDER) {
    const cells = Math.round((slices[key] / win) * height);
    const color = colorOf.get(key) ?? DIM;
    for (let k = 0; k < cells; k++) {
      const offset = filled + k;
      if (offset >= height) {
        break;
      }
      const row = rows[height - 1 - offset];
      if (row) {
        row[col] = { color };
      }
    }
    filled += cells;
  }
};

/** Categories with any nonzero token across the session, in stack order. */
const presentLegend = (s: AnalyzedSession): LegendEntry[] => {
  const meta = new Map(s.budget.map((b) => [b.key, b]));
  const nonzero = new Set<BudgetKey>();
  for (const snap of s.snapshots) {
    for (const key of STACK_ORDER) {
      if (snap.slices[key] > 0) {
        nonzero.add(key);
      }
    }
  }
  const out: LegendEntry[] = [];
  for (const key of STACK_ORDER) {
    if (!nonzero.has(key)) {
      continue;
    }
    const m = meta.get(key);
    out.push({
      key,
      label: m?.label ?? key,
      color: m?.color ?? DIM,
    });
  }
  return out;
};

/**
 * Build the terminal context-growth column chart for a session.
 *
 * @param s - the analyzed session (source of snapshots, window, budget)
 * @param opts - `width`/`height` in terminal cells for the plot grid
 */
export const buildGrowthChart = (
  s: AnalyzedSession,
  opts: { readonly width: number; readonly height: number }
): GrowthChart => {
  const win = s.contextWindow;
  const n = s.snapshots.length;
  const height = Math.max(1, Math.floor(opts.height));
  const maxW = Math.max(1, Math.floor(opts.width));
  if (win <= 0 || n === 0) {
    return emptyChart();
  }

  // Always fill the available width: `bucketColumns` downsamples when there are
  // more turns than columns and widens each turn into several columns when there
  // are fewer — so a short session still reads as a full-width chart.
  const cols = maxW;
  const columns = bucketColumns(s.snapshots, cols);
  const width = columns.length;
  const colorOf = new Map<BudgetKey, string>(
    s.budget.map((b) => [b.key, b.color])
  );

  const rows: Cell[][] = Array.from({ length: height }, () =>
    Array.from<Cell>({ length: width }).fill(null)
  );
  columns.forEach((column, col) => {
    paintColumn({ rows, col, slices: column.slices, win, height, colorOf });
  });

  const thresholdRaw = Math.round((1 - s.dumbZoneFraction) * height);
  const thresholdRow =
    thresholdRaw >= 0 && thresholdRaw < height ? thresholdRaw : -1;

  const yTicks: YTick[] = GRID_FRACTIONS.map((f) => ({
    row: Math.min(height - 1, Math.round((1 - f) * height)),
    label: fmtK(f * win),
  }));

  const step = Math.max(1, Math.ceil(width / MAX_X_TICKS));
  const xTicks: XTick[] = columns.flatMap((column, col) =>
    col % step === 0 ? [{ col, label: String(column.turn) }] : []
  );

  const peakCol = colOf(s.peakTurnIndex, n, cols);
  const compactionCols = s.compactionTurns.map((t) => colOf(t, n, cols));

  const peakFrac = win > 0 ? s.peakContextTokens / win : 0;
  const header = `${s.turnCount} turns · peak ${fmtK(
    s.peakContextTokens
  )} (${fmtPct(peakFrac)}) @ turn ${s.peakTurnIndex + 1}`;

  return {
    rows,
    width,
    height,
    yTicks,
    xTicks,
    thresholdRow,
    thresholdLabel: `DUMB ZONE ${Math.round(s.dumbZoneFraction * PERCENT)}%`,
    peakCol,
    compactionCols,
    legend: presentLegend(s),
    header,
    empty: false,
  };
};
