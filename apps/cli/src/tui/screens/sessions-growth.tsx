/** Terminal context-growth column chart pane.
 *
 * Renders `buildGrowthChart` as block-glyph columns: a header, the stacked plot
 * with a y-gutter of token labels and an overlaid dumb-zone boundary, an x-axis
 * of turn ticks with peak/compaction markers, and a wrapped category legend.
 * Same focus contract as `SessionHistory`: Left/Esc calls `onBack`; Tab is owned
 * by the parent and deliberately not handled here.
 */
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { useMemo } from "react";
import { Empty, Swatch } from "../components";
import { C, clip, sanitize } from "../theme";
import { buildGrowthChart, type Cell, type GrowthChart } from "./growth-chart";

/** Cells the left rail + gaps + this pane's chrome claim. */
const PANE_CHROME = 52;
/** Width of the y-axis gutter (right-aligned token labels). */
const Y_GUTTER = 8;
/** Non-plot terminal rows (nav, footer, borders, compact summary, toggle,
 * chart header, markers, axis, legend) to reserve when sizing the plot. */
const CHART_CHROME = 20;
/** Plot-height bounds in rows. */
const MIN_PLOT_H = 6;
const MAX_PLOT_H = 14;
/** `Context growth ` prefix length, reserved before the clipped header text. */
const HEADER_PREFIX_W = 15;
/** Minimum plot width in columns. */
const MIN_PLOT_W = 20;
/** Peak marker glyph. */
const PEAK_GLYPH = "◆";
/** Compaction marker glyph. */
const COMPACTION_GLYPH = "│";
/** Dashed glyph for the dumb-zone boundary over empty cells. */
const THRESHOLD_GLYPH = "╌";

/** A run of adjacent same-color cells, for run-length rendering. */
interface Run {
  readonly color: string | null;
  readonly key: string;
  readonly len: number;
}

/** Coalesce a row of cells into color runs so blocks render as one `<text>`. */
const runsOf = (cells: readonly Cell[], rowKey: string): Run[] => {
  const runs: Run[] = [];
  let start = 0;
  while (start < cells.length) {
    const color = cells[start]?.color ?? null;
    let end = start + 1;
    while (end < cells.length && (cells[end]?.color ?? null) === color) {
      end += 1;
    }
    runs.push({ key: `${rowKey}:${start}`, color, len: end - start });
    start = end;
  }
  return runs;
};

/** One rendered plot row: y label + run-length-encoded colored segments. */
const PlotRow = ({
  cells,
  rowIndex,
  yLabel,
  onThreshold,
}: {
  readonly cells: readonly Cell[];
  readonly rowIndex: number;
  readonly yLabel: string;
  readonly onThreshold: boolean;
}) => {
  const runs = runsOf(cells, `r${rowIndex}`);
  return (
    <box style={{ flexDirection: "row", height: 1, flexShrink: 0 }}>
      <text fg={C.textFaint}>{yLabel.padStart(Y_GUTTER)}</text>
      <text> </text>
      {runs.map((run) =>
        run.color === null ? (
          <text fg={onThreshold ? C.warn : C.border} key={run.key}>
            {(onThreshold ? THRESHOLD_GLYPH : " ").repeat(run.len)}
          </text>
        ) : (
          // Foreground full-block (not a bg-colored space) so the bars survive
          // terminals/tmux that strip background colors and stay copy-pasteable.
          <text fg={run.color} key={run.key}>
            {"█".repeat(run.len)}
          </text>
        )
      )}
    </box>
  );
};

/** The stacked plot grid with y gutter and dumb-zone boundary overlay. */
const Plot = ({ chart }: { readonly chart: GrowthChart }) => {
  const yByRow = new Map(chart.yTicks.map((t) => [t.row, t.label]));
  return (
    <box style={{ flexDirection: "column", flexShrink: 0, flexGrow: 0 }}>
      {chart.rows.map((cells, rowIndex) => (
        <PlotRow
          cells={cells}
          // biome-ignore lint/suspicious/noArrayIndexKey: rows are a fixed-height positional grid; the index is the row's identity
          key={`row:${rowIndex}`}
          onThreshold={rowIndex === chart.thresholdRow}
          rowIndex={rowIndex}
          yLabel={yByRow.get(rowIndex) ?? ""}
        />
      ))}
    </box>
  );
};

/** Marker row under the plot: peak diamond + compaction ticks per column. */
const MarkerRow = ({ chart }: { readonly chart: GrowthChart }) => {
  const compaction = new Set(chart.compactionCols);
  const glyphs = Array.from({ length: chart.width }, (_, col) => {
    if (col === chart.peakCol) {
      return { ch: PEAK_GLYPH, color: C.text };
    }
    if (compaction.has(col)) {
      return { ch: COMPACTION_GLYPH, color: C.warn };
    }
    return { ch: " ", color: C.textFaint };
  });
  return (
    <box style={{ flexDirection: "row", height: 1, flexShrink: 0 }}>
      <text>{" ".repeat(Y_GUTTER + 1)}</text>
      {glyphs.map((g, col) => (
        <text
          fg={g.color}
          // biome-ignore lint/suspicious/noArrayIndexKey: one glyph per fixed plot column; column index is its identity
          key={`mark:${col}`}
        >
          {g.ch}
        </text>
      ))}
    </box>
  );
};

/** X-axis line: turn-number ticks laid out at their columns. */
const AxisRow = ({ chart }: { readonly chart: GrowthChart }) => {
  const cells = Array.from({ length: chart.width }, () => " ");
  for (const tick of chart.xTicks) {
    for (let i = 0; i < tick.label.length; i++) {
      const col = tick.col + i;
      if (col < cells.length) {
        cells[col] = tick.label[i] ?? " ";
      }
    }
  }
  return (
    <box style={{ flexDirection: "row", height: 1, flexShrink: 0 }}>
      <text fg={C.textFaint}>{"turn".padStart(Y_GUTTER)}</text>
      <text> </text>
      <text fg={C.textDim}>{cells.join("")}</text>
    </box>
  );
};

/** Wrapped legend: a swatch + short label per present category. */
const Legend = ({ chart }: { readonly chart: GrowthChart }) => (
  <box style={{ flexDirection: "row", flexWrap: "wrap", gap: 2 }}>
    {chart.legend.map((entry) => (
      <box key={entry.key} style={{ flexDirection: "row" }}>
        <Swatch color={entry.color} />
        <text fg={C.textDim}>{` ${clip(sanitize(entry.label), 16)}`}</text>
      </box>
    ))}
  </box>
);

/** Right pane: the block-glyph context-growth column chart for one session. */
export const SessionGrowth = ({
  s,
  focused,
  onBack,
}: {
  readonly s: AnalyzedSession;
  readonly focused: boolean;
  readonly onBack: () => void;
}) => {
  const { width, height } = useTerminalDimensions();
  const plotW = Math.max(MIN_PLOT_W, width - PANE_CHROME - Y_GUTTER);
  const plotH = Math.max(
    MIN_PLOT_H,
    Math.min(MAX_PLOT_H, height - CHART_CHROME)
  );
  const chart = useMemo(
    () => buildGrowthChart(s, { width: plotW, height: plotH }),
    [s, plotW, plotH]
  );

  useKeyboard((key) => {
    if (!focused) {
      return;
    }
    if (key.name === "left" || key.name === "escape") {
      onBack();
    }
  });

  if (chart.empty) {
    return <Empty label="No turns with usage to plot." />;
  }

  const contentW = plotW + Y_GUTTER + 1;
  return (
    // A scrollbox so a short terminal scrolls the chart instead of the rows
    // overlapping — mirrors the history view; on a normal terminal it all fits.
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0 }}>
      <scrollbox focused={focused} style={{ flexGrow: 1, minHeight: 0 }}>
        <box style={{ flexDirection: "column", flexShrink: 0 }}>
          {/* One clipped line — a wrapping header would waste vertical space. */}
          <box style={{ flexDirection: "row", height: 1, flexShrink: 0 }}>
            <text fg={C.accent}>Context growth </text>
            <text fg={C.textDim}>
              {clip(
                `${chart.header} · ${chart.thresholdLabel}`,
                Math.max(10, contentW - HEADER_PREFIX_W - 3)
              )}
            </text>
          </box>
          <text> </text>
          <Plot chart={chart} />
          <MarkerRow chart={chart} />
          <AxisRow chart={chart} />
          <text> </text>
          <Legend chart={chart} />
        </box>
      </scrollbox>
    </box>
  );
};
