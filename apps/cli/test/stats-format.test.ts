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
  CauseRow,
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { DETECTOR_VERSION } from "@workspace/core/services/stats/schema";
import {
  columnsOf,
  findingsOf,
  findingTarget,
  layoutOf,
  noteLinesOf,
  openCommand,
  refsOf,
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
    schemaVersion: "peektrace-stats/v2",
    generatedAt: 0,
    window: { days: 30, fromIso: "", toIso: "" },
    detectorVersion: DETECTOR_VERSION,
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
    causes: [],
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
    paths: {},
    caveats: [],
    ...over,
  }) as StatsReport;

test("findings rank by effort tier, never by a blended score", () => {
  const r = report({
    causes: [],
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
    causes: [],
    fails: [failRow({ note: null, rows: 7 })],
    time: [timeRow({ cappedRuns: 2, cappedSec: 30 })],
  });
  const noRule = noteLinesOf({
    findings: [],
    index: 0,
    pane: "fails",
    report: r,
  });
  expect(noRule[0]).toContain("no rule to suggest");
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

test("a subagent call routes through its parent, never its own id", () => {
  const parent = "3f2a1b4c-9d8e-4f00-8000-000000000000";
  const sub = {
    sid: "agent-aac481ccea1ab4b5a",
    agent: "claude",
    project: "p",
    kind: "subagent",
    parent,
    seq: 3,
    line: "x | tail",
  } as const;
  expect(refsOf([sub])).toBe("3f2a1b4c/sub");
  expect(openCommand(sub)).toBe(
    `peektrace sessions analyze --parent ${parent} agent-aac481ccea1ab4b5a`
  );
  expect(openCommand({ sid: parent, parent: null })).toBe(
    `peektrace sessions analyze ${parent}`
  );
});

const causeRow = (over: Partial<CauseRow>): CauseRow => ({
  key: "cause:zsh-glob-flag",
  cause: "zsh-glob-flag",
  kind: "rule",
  label: "unquoted glob in a flag value",
  fix: "quote the glob",
  failing: true,
  hits: 40,
  flagged: 2,
  hitsBy: { main: 10, nested: 30 },
  pairs: 12,
  pairsBy: { main: 2, nested: 10 },
  retrySessions: 11,
  sessions: 30,
  projects: 3,
  retrySec: 50,
  secMeasured: 12,
  retryCtxTokens: 9000,
  tokensMeasured: 12,
  per1k: { main: 1, nested: 2 },
  details: [],
  diffs: [],
  rule: null,
  samples: [],
  pairSamples: [],
  ...over,
});

test("a cause finding leads and drills into the cause table", () => {
  const r = report({
    causes: [causeRow({})],
    fails: [failRow({ key: "named", note: "x", detector: "piped-exit-code" })],
  });
  const found = findingsOf(r);
  expect(found.map((f) => f.key)).toEqual(["cause:zsh-glob-flag", "named"]);
  const first = found[0];
  const target = first === undefined ? null : findingTarget(first, r);
  expect(target?.table).toBe("cause");
  expect(target?.key).toBe("cause:zsh-glob-flag");
  expect(target?.note).toBe("quote the glob");
  expect(target?.spread).toContain("11 sessions with a retry");
  expect(target?.spread).toContain("12 retries (main");
});

test("a cause finding spells re-read context compactly, as the inspector does", () => {
  const r = report({ causes: [causeRow({ retryCtxTokens: 32_389_802 })] });
  const [first] = findingsOf(r);
  expect(first?.impact).toBe(
    "12 retries (main 2 / sub 10) · 50.0s · 32.4M ctx tokens re-read"
  );
});
