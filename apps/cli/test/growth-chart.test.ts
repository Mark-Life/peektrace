/** Unit tests for the pure growth-chart builder. */
import { expect, test } from "bun:test";
import type {
  AnalyzedSession,
  BudgetKey,
  BudgetSlices,
  TurnSnapshot,
} from "@workspace/core/services/sessions/schema";
import { buildGrowthChart, STACK_ORDER } from "../src/tui/screens/growth-chart";

const ZERO: BudgetSlices = {
  system_tools: 0,
  listings: 0,
  memory: 0,
  files: 0,
  prompts: 0,
  tool_results: 0,
  assistant_text: 0,
  thinking: 0,
  other: 0,
  unattributed: 0,
};

/** A snapshot whose whole ctx sits in one category (default: files). */
const snap = (
  ctx: number,
  key: BudgetKey = "files",
  turnIndex = 0
): TurnSnapshot => ({
  turnIndex,
  model: "m",
  ctx,
  outputTokens: 0,
  cacheReadTokens: 0,
  slices: { ...ZERO, [key]: ctx },
});

/** Minimal analyzed session over the given snapshots. */
const session = (
  snapshots: TurnSnapshot[],
  over: Partial<AnalyzedSession> = {}
): AnalyzedSession =>
  ({
    contextWindow: 1000,
    dumbZoneFraction: 0.6,
    peakTurnIndex: 0,
    peakContextTokens: Math.max(0, ...snapshots.map((s) => s.ctx)),
    compactionTurns: [],
    turnCount: snapshots.length,
    snapshots,
    budget: STACK_ORDER.map((key) => ({
      key,
      label: `L-${key}`,
      short: key,
      tokens: snapshots.reduce((sum, s) => sum + s.slices[key], 0),
      color: `#${key.length}00000`,
      estimated: false,
    })),
    ...over,
  }) as unknown as AnalyzedSession;

/** Count painted (non-null) cells in a column. */
const columnFill = (chart: ReturnType<typeof buildGrowthChart>, col: number) =>
  chart.rows.reduce((count, row) => (row[col] ? count + 1 : count), 0);

test("column fill scales with ctx / window", () => {
  const chart = buildGrowthChart(session([snap(250), snap(500)]), {
    width: 10,
    height: 20,
  });
  // Columns fill the full width: snapshot 0 (250/1000 * 20 = 5) occupies the
  // left half, snapshot 1 (500/1000 * 20 = 10) the right half.
  expect(chart.width).toBe(10);
  expect(columnFill(chart, 0)).toBe(5);
  expect(columnFill(chart, chart.width - 1)).toBe(10);
});

test("a full-window snapshot fills every row", () => {
  const chart = buildGrowthChart(session([snap(1000)]), {
    width: 8,
    height: 16,
  });
  expect(columnFill(chart, 0)).toBe(16);
});

test("thresholdRow reflects dumbZoneFraction", () => {
  const chart = buildGrowthChart(
    session([snap(500)], { dumbZoneFraction: 0.75 }),
    { width: 4, height: 20 }
  );
  // round((1 - 0.75) * 20) = 5
  expect(chart.thresholdRow).toBe(5);
});

test("buckets when snapshots exceed width", () => {
  const many = Array.from({ length: 50 }, (_, i) => snap(100 + i, "files", i));
  const chart = buildGrowthChart(session(many), { width: 10, height: 20 });
  expect(chart.width).toBe(10);
  expect(chart.rows[0]?.length).toBe(10);
});

test("legend only includes nonzero categories", () => {
  const chart = buildGrowthChart(
    session([snap(400, "files"), snap(300, "thinking")]),
    { width: 6, height: 12 }
  );
  const keys = chart.legend.map((e) => e.key);
  expect(keys).toEqual(["files", "thinking"]);
  expect(keys).not.toContain("memory");
});

test("empty session yields an empty chart", () => {
  const chart = buildGrowthChart(session([]), { width: 10, height: 10 });
  expect(chart.empty).toBe(true);
  expect(chart.rows).toHaveLength(0);
});

test("zero context window yields an empty chart", () => {
  const chart = buildGrowthChart(session([snap(500)], { contextWindow: 0 }), {
    width: 10,
    height: 10,
  });
  expect(chart.empty).toBe(true);
});

test("peakCol maps the peak turn into the bucketed grid", () => {
  const many = Array.from({ length: 40 }, (_, i) => snap(100, "files", i));
  const chart = buildGrowthChart(session(many, { peakTurnIndex: 20 }), {
    width: 10,
    height: 12,
  });
  // floor(20 * 10 / 40) = 5
  expect(chart.peakCol).toBe(5);
});
