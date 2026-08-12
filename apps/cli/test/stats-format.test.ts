/** Unit tests for the stats screen's pure layout and ranking helpers.
 *
 * The findings tier order is repeated from the web inspector, so it is pinned
 * here on a fixture: a fail row with a named detector leads, then a fail row
 * with a note and no detector, then a time row. The two tables are never blended
 * into one score, and the column budget must add up to the content width at both
 * the wide and the narrow layout.
 */
import { expect, test } from "bun:test";
import type {
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import {
  columnsOf,
  findingsOf,
  layoutOf,
  noteLinesOf,
  shareCell,
  wrap,
} from "../src/tui/screens/stats-format";

const SPACES = /\s+/;

const failRow = (over: Partial<FailRow>): FailRow => ({
  key: "k",
  label: "label",
  where: "Bash",
  rows: 10,
  sessions: 5,
  projects: 2,
  flagged: 0,
  note: null,
  detector: null,
  samples: [],
  ...over,
});

const timeRow = (over: Partial<TimeRow>): TimeRow => ({
  key: "test",
  label: "tests",
  runs: 10,
  coPresentRuns: 12,
  totalSec: 3600,
  share: 0.5,
  medianSec: 1,
  p95Sec: 9,
  cappedRuns: 0,
  cappedSec: 0,
  bgRuns: 0,
  bgSec: 0,
  samples: [],
  ...over,
});

const report = (over: Partial<StatsReport>): StatsReport =>
  ({
    schemaVersion: "peektrace-stats/v1",
    generatedAt: 0,
    window: { days: 30, fromIso: "", toIso: "" },
    detectorVersion: 1,
    corpus: {
      transcripts: 1,
      sessions: 1,
      projects: 1,
      toolCalls: 1,
      bashCalls: 1,
      bashSec: 100,
      toolSec: 100,
      humanWaitSec: 0,
      kinds: {},
      agents: {},
    },
    fails: [],
    time: [],
    waiting: {
      calls: 0,
      totalSec: 0,
      share: 0,
      sessions: 0,
      projects: 0,
      medianSec: 0,
      p95Sec: 0,
      byShape: [],
      bgRuns: 0,
      bgSec: 0,
      bgDeclaredSec: 0,
      samples: [],
    },
    clusters: [],
    context: [],
    fanout: [],
    web: [],
    cost: [],
    skipped: [],
    caveats: [],
    ...over,
  }) as StatsReport;

test("findings rank by effort tier, never by a blended score", () => {
  const r = report({
    fails: [
      failRow({ key: "plain", label: "plain", note: "measured", sessions: 99 }),
      failRow({
        key: "named",
        label: "named",
        note: "quote the separator",
        detector: "zsh-equals-abort",
        sessions: 1,
      }),
      failRow({ key: "quiet", label: "quiet", note: null, sessions: 500 }),
    ],
    time: [timeRow({ cappedRuns: 3, cappedSec: 60 })],
  });
  expect(findingsOf(r).map((f) => f.key)).toEqual(["named", "plain", "test"]);
});

test("a row core minted no note for is never a finding", () => {
  const r = report({ fails: [failRow({ note: null })], time: [timeRow({})] });
  expect(findingsOf(r)).toHaveLength(0);
});

test("fail columns fill the content width exactly, both layouts", () => {
  const gutter = 2;
  for (const content of [98, 78]) {
    const { fail } = columnsOf(content, true);
    const gaps = fail.where > 0 ? 5 : 4;
    const used =
      gutter +
      fail.sessions +
      fail.calls +
      fail.flagged +
      fail.proj +
      fail.where +
      fail.what +
      gaps * fail.gap.length;
    expect(used).toBe(content);
  }
});

test("time columns fill the content width exactly, both layouts", () => {
  const gutter = 2;
  for (const content of [98, 78]) {
    const { time } = columnsOf(content, true);
    const gaps = time.median > 0 ? 8 : 6;
    const used =
      gutter +
      time.job +
      time.time +
      time.share +
      time.runs +
      time.coPresent +
      time.median +
      time.p95 +
      time.capped +
      time.bg +
      gaps * time.gap.length;
    expect(used).toBe(content);
  }
});

test("an unmeasurable share takes its room from the job column", () => {
  const measured = columnsOf(98, true).time;
  const not = columnsOf(98, false).time;
  expect(shareCell(0.5, 0)).toBe("not measured");
  expect(not.share).toBe(measured.share + "not measured".length - 6);
  expect(not.job).toBe(measured.job - (not.share - measured.share));
});

test("the narrow layout drops WHERE, MEDIAN and P95", () => {
  const narrow = columnsOf(78, true);
  expect(narrow.wide).toBe(false);
  expect(narrow.fail.where).toBe(0);
  expect(narrow.time.median).toBe(0);
  expect(narrow.time.p95).toBe(0);
});

test("layout gives both tables a body and sheds the note strip last", () => {
  expect(layoutOf(44)).toEqual({
    failRows: 8,
    findings: 3,
    noteLines: 4,
    timeRows: 7,
  });
  const short = layoutOf(24);
  expect(short.findings).toBe(0);
  expect(short.noteLines).toBe(0);
  expect(short.failRows).toBeGreaterThanOrEqual(3);
  expect(short.timeRows).toBeGreaterThanOrEqual(3);
});

test("the note strip states the measurement, never invented advice", () => {
  const r = report({
    fails: [failRow({ note: null, rows: 7 })],
    time: [timeRow({ cappedRuns: 2, cappedSec: 30 })],
  });
  const noRule = noteLinesOf({
    findings: [],
    index: 0,
    pane: "fails",
    report: r,
  });
  expect(noRule[0]).toContain("no rule fired");
  const capped = noteLinesOf({
    findings: [],
    index: 0,
    pane: "time",
    report: r,
  });
  expect(capped[0]).toContain("floor");
  expect(capped[1]).toContain("median");
});

test("wrap keeps every word and hangs the continuation", () => {
  const lines = wrap("- one two three four five six seven eight", 16);
  expect(lines.join(" ").split(SPACES).filter(Boolean)).toEqual(
    "- one two three four five six seven eight".split(" ")
  );
  expect(lines.length).toBeGreaterThan(1);
  expect(lines[1]?.startsWith("  ")).toBe(true);
});
