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

import type {
  CauseRow,
  FailRow,
  RuleCheck,
  StatsReport,
  TimeRow,
} from "@workspace/view-core/api-types.gen";
import { NO_RULE } from "@workspace/view-core/constants.gen";

import {
  causeNote,
  findingsOf,
  rateText,
  ruleLine,
  timeNote,
  timeSpread,
} from "../src/lib/stats-findings";
import type { FindingFormat } from "../src/lib/stats-findings";
import { report as emptyReport } from "./stats-report-fixture";

const FORMAT: FindingFormat = {
  count: String,
  hms: (sec) => `${sec}s`,
  pct: (share) => `${share * 100}%`,
  // Bracketed, so a test sees the token count took this path, not count().
  tokens: (value) => `<${value}>`,
};

const failRow = (over: Partial<FailRow>): FailRow => ({
  detector: null,
  flagged: 0,
  key: "k",
  label: "label",
  note: null,
  projects: 2,
  rows: 10,
  samples: [],
  sessions: 5,
  where: "Bash",
  ...over,
});

const timeRow = (over: Partial<TimeRow>): TimeRow => ({
  bgRuns: 0,
  bgSec: 0,
  cappedRuns: 0,
  cappedSec: 0,
  coPresentRuns: 12,
  key: "test",
  label: "tests",
  medianSec: 1,
  p95Sec: 9,
  runs: 10,
  samples: [],
  share: 0.5,
  totalSec: 3600,
  ...over,
});

const causeRow = (over: Partial<CauseRow>): CauseRow => ({
  cause: "zsh-glob-flag",
  details: [],
  diffs: [],
  failing: true,
  fix: "quote the glob",
  flagged: 2,
  hits: 40,
  hitsBy: { main: 10, nested: 30 },
  key: "cause:zsh-glob-flag",
  kind: "rule",
  label: "unquoted glob in a flag value",
  pairSamples: [],
  pairs: 12,
  pairsBy: { main: 2, nested: 10 },
  per1k: { main: 1, nested: 2 },
  projects: 3,
  retryCtxTokens: 900,
  retrySec: 50,
  retrySessions: 11,
  rule: null,
  samples: [],
  secMeasured: 12,
  sessions: 30,
  tokensMeasured: 12,
  ...over,
});

const report = (over: {
  readonly bashSec?: number;
  readonly causes?: readonly CauseRow[];
  readonly fails?: readonly FailRow[];
  readonly time?: readonly TimeRow[];
}): StatsReport => {
  const base = emptyReport({ fails: over.fails ?? [], time: over.time ?? [] });

  return {
    ...base,
    causes: over.causes ?? [],
    corpus: { ...base.corpus, bashSec: over.bashSec ?? 100 },
  };
};

describe("findingsOf", () => {
  test("a named rule leads, then a measured note, then the time rows", () => {
    const r = report({
      fails: [
        failRow({ key: "plain", note: "printed a failure", sessions: 9 }),
        failRow({
          detector: "zsh-equals-abort",
          key: "named",
          note: "quote the separator",
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

describe("cause findings", () => {
  test("cause findings lead, in the cause table's order", () => {
    const r = report({
      causes: [
        causeRow({}),
        causeRow({ cause: "pkill-self-kill", key: "cause:pkill-self-kill" }),
      ],
      fails: [
        failRow({ detector: "piped-exit-code", key: "named", note: "x" }),
      ],
      time: [timeRow({ cappedRuns: 3, cappedSec: 60 })],
    });

    expect(findingsOf(r, FORMAT).map((f) => [f.table, f.key])).toEqual([
      ["cause", "cause:zsh-glob-flag"],
      ["cause", "cause:pkill-self-kill"],
      ["fail", "named"],
      ["time", "test"],
    ]);
  });

  test("a cause finding states its retries, never its hits, as the cost", () => {
    const [finding] = findingsOf(report({ causes: [causeRow({})] }), FORMAT);
    expect(finding?.impact).toBe(
      "12 retries (main 2 / sub 10) · 50s · <900> ctx tokens re-read"
    );
    expect(finding?.spread).toBe(
      "40 hits (38 at exit 0) · 11 sessions with a retry · 3 projects · hits main 10 / sub 30"
    );
  });

  test("a cause with no measured retry gap states no time, not 0s", () => {
    const [finding] = findingsOf(
      report({ causes: [causeRow({ retrySec: 0, secMeasured: 0 })] }),
      FORMAT
    );

    expect(finding?.impact).toBe(
      "12 retries (main 2 / sub 10) · <900> ctx tokens re-read"
    );
  });

  test("a fix-less row is not a finding, and the table spells it out", () => {
    const r = report({
      causes: [
        causeRow({
          cause: "unclassified",
          fix: null,
          key: "cause:unclassified:Bash",
          kind: "none",
        }),
      ],
    });

    expect(findingsOf(r, FORMAT)).toHaveLength(0);
    expect(NO_RULE).toBe("no rule to suggest");
  });

  test("a rule already present and still firing says so instead of the fix", () => {
    const rule: RuleCheck = {
      advice: "setopt nonomatch",
      callsSince: { main: 100, nested: 100 },
      hitsSince: { main: 1, nested: 9 },
      matches: [
        {
          excerpt: "Quote globs",
          line: 100,
          path: "~/.claude/CLAUDE.md",
        },
      ],
      per1kSince: { main: 10, nested: 90 },
      sinceIso: "2026-08-01T00:00:00.000Z",
    };

    const row = causeRow({ rule });

    expect(causeNote(row)).toBe(
      "rule in ~/.claude/CLAUDE.md:100 · still firing main 10/1k, sub 90/1k since 2026-08-01: setopt nonomatch"
    );
    expect(
      ruleLine({
        ...rule,
        callsSince: { main: 0, nested: 100 },
      })
    ).toBe(
      "rule in ~/.claude/CLAUDE.md:100 · still firing sub 90/1k since 2026-08-01"
    );
    expect(
      ruleLine({
        ...rule,
        hitsSince: { main: 0, nested: 0 },
      })
    ).toBe("rule in ~/.claude/CLAUDE.md:100 · quiet since 2026-08-01");

    // Changed on the last day: nothing measured yet, which is not "quiet".
    const fresh = causeRow({
      rule: {
        ...rule,
        advice: "if it keeps firing: setopt nonomatch",
        callsSince: { main: 0, nested: 0 },
        hitsSince: { main: 0, nested: 0 },
        per1kSince: { main: 0, nested: 0 },
      },
    });

    expect(causeNote(fresh)).toBe(
      "rule in ~/.claude/CLAUDE.md:100 · not measured yet (added 2026-08-01): if it keeps firing: setopt nonomatch"
    );
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

describe("rateText", () => {
  test("one spelling of a per-1k rate, whatever a surface's count rounds to", () => {
    expect(rateText(41.46, 5)).toBe("41");
    expect(rateText(4.236, 5)).toBe("4.2");
    expect(rateText(0.31, 1)).toBe("0.31");
    expect(rateText(0, 0)).toBe("0");
  });

  test("a side with hits never reads 0/1k", () => {
    expect(rateText(0, 1)).toBe("<0.01");

    const line = ruleLine({
      advice: "a",
      callsSince: { main: 10, nested: 4000 },
      hitsSince: { main: 0, nested: 1 },
      matches: [{ excerpt: "x", line: 1, path: "~/.claude/CLAUDE.md" }],
      per1kSince: { main: 0, nested: 0.25 },
      sinceIso: "2026-08-01T00:00:00.000Z",
    });

    expect(line).toContain("still firing main 0/1k, sub 0.25/1k");
  });
});
