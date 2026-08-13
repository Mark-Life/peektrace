/** The cold-start list, which every surface reads from here.
 *
 * Two properties carry it. The tier order is the claim — a named rule before a
 * measured note before a time row — and it must hold whatever order the tables
 * arrive in. And a share of nothing must not print as zero percent: a corpus
 * with no shell seconds has no share to state, so it says so.
 *
 * The formatters are stand-ins. What is pinned is which rows qualify, in what
 * order, and which sentence each one takes.
 */

import { describe, expect, test } from "bun:test";
import {
  type FindingFormat,
  findingsOf,
  timeNote,
  timeSpread,
} from "../../src/services/stats/findings";
import type {
  FailRow,
  StatsReport,
  TimeRow,
} from "../../src/services/stats/schema";

const FORMAT: FindingFormat = {
  count: (value) => String(value),
  hms: (sec) => `${sec}s`,
  pct: (share) => `${share * 100}%`,
};

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

const report = (over: {
  readonly bashSec?: number;
  readonly fails?: readonly FailRow[];
  readonly time?: readonly TimeRow[];
}): StatsReport =>
  ({
    corpus: { bashSec: over.bashSec ?? 100 },
    fails: over.fails ?? [],
    time: over.time ?? [],
  }) as unknown as StatsReport;

describe("findingsOf", () => {
  test("a named rule leads, then a measured note, then the time rows", () => {
    const r = report({
      fails: [
        failRow({ key: "plain", note: "printed a failure", sessions: 9 }),
        failRow({
          key: "named",
          note: "quote the separator",
          detector: "zsh-equals-abort",
        }),
        failRow({ key: "quiet", note: null }),
      ],
      time: [timeRow({ cappedRuns: 3, cappedSec: 60 })],
    });
    expect(findingsOf(r, FORMAT).map((f) => f.key)).toEqual([
      "named",
      "plain",
      "test",
    ]);
  });

  test("a row core minted no note for is never a finding", () => {
    const r = report({ fails: [failRow({})], time: [timeRow({})] });
    expect(findingsOf(r, FORMAT)).toHaveLength(0);
  });

  test("the two tables keep their own units and are never added", () => {
    const r = report({
      fails: [failRow({ note: "printed a failure", sessions: 9 })],
      time: [timeRow({ cappedRuns: 3, cappedSec: 60 })],
    });
    const [fail, time] = findingsOf(r, FORMAT);
    expect(fail?.impact).toBe("9 sessions");
    expect(time?.impact).toBe("3600s");
  });
});

describe("timeSpread", () => {
  test("a share of no shell seconds says so instead of printing zero", () => {
    expect(timeSpread(timeRow({}), 0, FORMAT)).toBe("not measured · 10 runs");
    expect(timeSpread(timeRow({}), 100, FORMAT)).toBe(
      "50% of shell time · 10 runs"
    );
  });
});

describe("timeNote", () => {
  test("a ceiling is stated as a floor on the seconds, not as a duration", () => {
    const note = timeNote(timeRow({ cappedRuns: 3, cappedSec: 60 }), FORMAT);
    expect(note).toContain("harness ceiling");
    expect(note).toContain("these seconds are a floor");
  });

  test("a row with nothing measured about it says nothing", () => {
    expect(timeNote(timeRow({}), FORMAT)).toBeNull();
  });
});
