/** The plain-text `peektrace stats` report: findings lead, the cause table
 * comes before the fail table, and every missing fix says so in words. */
import { expect, test } from "bun:test";
import { suggestedRules } from "@workspace/core/services/stats/findings";
import type {
  CauseRow,
  FailRow,
  StatsReport,
} from "@workspace/core/services/stats/schema";
import { DETECTOR_VERSION } from "@workspace/core/services/stats/schema";
import { renderReport } from "../src/commands/stats";

const causeRow = (over: Partial<CauseRow>): CauseRow => ({
  key: "cause:zsh-glob-flag",
  cause: "zsh-glob-flag",
  kind: "rule",
  label: "unquoted glob in a flag value",
  fix: "quote every glob passed as a flag value",
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

const failRow = (over: Partial<FailRow>): FailRow => ({
  key: "flag:Bash",
  label: "Bash errors",
  where: "Bash",
  rows: 3,
  sessions: 3,
  projects: 1,
  flagged: 3,
  note: null,
  detector: null,
  samples: [],
  ...over,
});

const report = (over: Partial<StatsReport>): StatsReport =>
  ({
    schemaVersion: "peektrace-stats/v2",
    generatedAt: 0,
    window: { days: 30, fromIso: "a", toIso: "b" },
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
    caveats: [],
    ...over,
  }) as StatsReport;

const STILL_FIRING = causeRow({
  key: "cause:zsh-equals-abort",
  cause: "zsh-equals-abort",
  label: "unquoted echo === separator",
  fix: 'quote the separator: echo "==="',
  rule: {
    matches: [{ path: "~/.claude/CLAUDE.md", line: 99, excerpt: "x" }],
    sinceIso: "2026-08-01T00:00:00.000Z",
    hitsSince: { main: 1, nested: 3 },
    callsSince: { main: 500, nested: 500 },
    per1kSince: { main: 2, nested: 6 },
    advice: "unsetopt equals",
  },
});

test("findings lead, then the cause table, then the fail table", () => {
  const text = renderReport(
    report({
      causes: [causeRow({}), STILL_FIRING],
      fails: [failRow({})],
    }),
    true
  );
  const at = (s: string) => text.indexOf(s);
  expect(at("FIX FIRST")).toBeGreaterThan(0);
  expect(at("FIX FIRST")).toBeLessThan(at("AVOIDABLE RETRIES"));
  expect(at("AVOIDABLE RETRIES")).toBeLessThan(at("WHAT FAILS"));
  expect(text).toContain("--cause cause:zsh-glob-flag");
  expect(text).toContain(
    "rule in ~/.claude/CLAUDE.md:99 · still firing main 2/1k, sub 6/1k since 2026-08-01: unsetopt equals"
  );
  // The split counts hits, and the table's rate names its window apart from
  // the rule line's rate since the file changed.
  expect(text).toContain("\thits m/s\tper 1k in window m/s\t");
  // Re-read context is spelled compactly, in the findings and the table.
  expect(text).toContain("9.0k ctx tokens re-read");
  expect(text).not.toContain("9,000");
  // A fail row with no fix says so, never a bare dash.
  expect(text).toContain("no rule to suggest");
});

test("suggested rules: rule causes no memory file states yet, nothing else", () => {
  const r = report({
    causes: [
      causeRow({}),
      STILL_FIRING,
      causeRow({
        key: "cause:command-not-found",
        cause: "command-not-found",
        kind: "env",
        fix: "install jq",
      }),
      causeRow({
        key: "cause:unclassified:Bash",
        cause: "unclassified",
        kind: "none",
        fix: null,
      }),
    ],
  });
  expect(suggestedRules(r)).toEqual([
    "quote every glob passed as a flag value",
  ]);
  expect(renderReport(r, true)).toContain(
    "SUGGESTED RULES  (copy into CLAUDE.md / AGENTS.md; causes already stated are not repeated)\n- quote every glob passed as a flag value"
  );
});

test("no suggestion says whether nothing fired or all of it is stated", () => {
  expect(renderReport(report({ causes: [] }), true)).toContain(
    "SUGGESTED RULES  none: no rule cause fired in this window"
  );
  expect(renderReport(report({ causes: [STILL_FIRING] }), true)).toContain(
    "SUGGESTED RULES  none: every rule cause that fired is already stated in a memory file"
  );
});
