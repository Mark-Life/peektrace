/** The two rules the stats screen owns: how a second reads, and what leads.
 *
 * The findings order is the one piece of judgement in the UI, so it is pinned
 * here rather than left to the JSX: a row whose detector names a one-line fix
 * outranks a row that only carries a measured note, which outranks a time row —
 * and a fail count is never added to a second.
 */
import { expect, test } from "bun:test";
import type {
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { findingsOf, hms, timeNote } from "../src/lib/stats-format";

const failRow = (
  over: Partial<FailRow> & { readonly key: string }
): FailRow => ({
  detector: null,
  flagged: 0,
  label: over.key,
  note: null,
  projects: 1,
  rows: 1,
  samples: [],
  sessions: 1,
  where: "Bash exit 0",
  ...over,
});

const timeRow = (
  over: Partial<TimeRow> & { readonly key: TimeRow["key"] }
) => ({
  bgRuns: 0,
  bgSec: 0,
  cappedRuns: 0,
  cappedSec: 0,
  coPresentRuns: 0,
  label: over.key,
  medianSec: 1,
  p95Sec: 2,
  runs: 10,
  samples: [],
  share: 0.1,
  totalSec: 100,
  ...over,
});

const report = (args: {
  readonly fails: readonly FailRow[];
  readonly time: readonly TimeRow[];
}): StatsReport => ({
  caveats: [],
  clusters: [],
  context: [],
  corpus: {
    agents: {},
    bashCalls: 0,
    bashSec: 0,
    kinds: {},
    projects: 0,
    sessions: 0,
    toolCalls: 0,
    toolSec: 0,
    humanWaitSec: 0,
    transcripts: 0,
  },
  cost: [],
  detectorVersion: 1,
  fails: args.fails,
  fanout: [],
  generatedAt: 0,
  schemaVersion: "peektrace-stats/v1",
  skipped: [],
  time: args.time,
  waiting: {
    bgDeclaredSec: 0,
    bgRuns: 0,
    bgSec: 0,
    byShape: [],
    calls: 0,
    medianSec: 0,
    p95Sec: 0,
    projects: 0,
    samples: [],
    sessions: 0,
    share: 0,
    totalSec: 0,
  },
  web: [],
});

test("seconds read as a duration, never as a bare float", () => {
  expect(hms(0.4)).toBe("0.40s");
  expect(hms(42)).toBe("42s");
  expect(hms(187)).toBe("3m07s");
  expect(hms(18_277)).toBe("5h04m");
});

test("a time row says only what its own columns measured", () => {
  expect(timeNote(timeRow({ key: "typecheck" }))).toBeNull();
  expect(
    timeNote(timeRow({ bgRuns: 3, cappedRuns: 2, cappedSec: 60, key: "test" }))
  ).toContain("harness ceiling");
  expect(timeNote(timeRow({ bgRuns: 3, key: "wait/poll" }))).toContain(
    "backgrounded"
  );
});

test("findings lead with the cheapest fix, then each table's own impact", () => {
  const findings = findingsOf(
    report({
      fails: [
        failRow({
          key: "scan:tsc",
          note: "555 of 576 pipe into head/tail",
          sessions: 225,
        }),
        failRow({
          detector: "zsh-equals-abort",
          key: "detect:zsh-equals-abort",
          note: 'quote the separator: echo "==="',
          sessions: 91,
        }),
        failRow({ key: "flag:Bash", sessions: 300 }),
      ],
      time: [timeRow({ cappedRuns: 4, cappedSec: 300, key: "test" })],
    })
  );
  expect(findings.map((f) => f.key)).toEqual([
    "detect:zsh-equals-abort",
    "scan:tsc",
    "test",
  ]);
  // The unnoted flag row outranks both on sessions and is still not a finding:
  // a finding is a row that carries something to do about it.
  expect(findings.some((f) => f.key === "flag:Bash")).toBe(false);
  expect(findings[0]?.impact).toBe("91 sessions");
  expect(findings.at(-1)?.impact).toBe("1m40s");
});
