/** The plain-text `peektrace stats usage` view, over synthetic reports. */
import { describe, expect, test } from "bun:test";
import type {
  UsageReport,
  UsageRow,
} from "@workspace/core/services/stats/usage-schema";
import { renderUsage } from "../src/commands/stats-usage";

const DAY_HEADER =
  /^DATE\s+INPUT\s+OUTPUT\s+CACHE W\s+CACHE R\s+TOTAL\s+COST\s+MODELS$/;
const BLOCK_HEADER = /BLOCK START\s+END\s+ACTIVE/;
const KOLKATA_BLOCK = /2026-08-01 15:30\s+20:30\s+\*/;

const row = (over: Partial<UsageRow>): UsageRow => ({
  key: "2026-08-01",
  turns: 3,
  inputTokens: 1200,
  outputTokens: 300,
  cacheReadTokens: 2_500_000,
  cacheCreationTokens: 40_000,
  totalTokens: 2_541_500,
  usd: 1.234,
  models: ["claude-sonnet-4-5"],
  block: null,
  ...over,
});

const report = (over: Partial<UsageReport>): UsageReport => ({
  schemaVersion: "peektrace-stats-usage/v1",
  generatedAt: 0,
  by: "day",
  tz: "UTC",
  window: { days: 30, fromIso: "", toIso: "" },
  rows: [row({})],
  totals: {
    turns: 3,
    inputTokens: 1200,
    outputTokens: 300,
    cacheReadTokens: 2_500_000,
    cacheCreationTokens: 40_000,
    totalTokens: 2_541_500,
    usd: 1.234,
    models: ["claude-sonnet-4-5"],
  },
  unpricedModels: [],
  skipped: { noTimestamp: 0, zeroTokens: 0, duplicates: 0, otherAgents: 0 },
  caveats: ["Cost is an ESTIMATE"],
  ...over,
});

describe("renderUsage", () => {
  test("day rows carry the token columns, the cost and a totals line", () => {
    const out = renderUsage(report({}), false);
    const lines = out.split("\n");
    expect(lines[1]).toMatch(DAY_HEADER);
    expect(out).toContain("2026-08-01");
    expect(out).toContain("2.5M");
    expect(out).toContain("~$1.23");
    expect(lines.some((l) => l.startsWith("TOTAL "))).toBe(true);
    expect(out).toContain("NOTES\n- Cost is an ESTIMATE");
  });

  test("block rows print start and end in the report's zone and mark the open one", () => {
    const out = renderUsage(
      report({
        by: "block",
        tz: "Asia/Kolkata",
        rows: [
          row({
            key: "2026-08-01T10:00:00.000Z",
            block: {
              startIso: "2026-08-01T10:00:00.000Z",
              endIso: "2026-08-01T15:00:00.000Z",
              lastIso: "2026-08-01T11:00:00.000Z",
              active: true,
            },
          }),
        ],
      }),
      false
    );
    expect(out).toMatch(BLOCK_HEADER);
    expect(out).toMatch(KOLKATA_BLOCK);
  });

  test("a long model list names three and counts the rest", () => {
    const out = renderUsage(
      report({ rows: [row({ models: ["a", "b", "c", "d", "e"] })] }),
      false
    );
    expect(out).toContain("a,b,c +2");
  });

  test("compact mode is tab-separated, and an empty view has no totals", () => {
    const out = renderUsage(report({ rows: [] }), true);
    expect(out).toContain("date\tinput\toutput");
    expect(out).toContain("(none)");
    expect(out).not.toContain("TOTAL\t");
  });
});
