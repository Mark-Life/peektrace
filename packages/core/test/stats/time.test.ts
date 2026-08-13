/** The time table and the waiting panel.
 *
 * Three properties decide whether the table means anything. Every bash line is
 * charged to exactly one job, so the shares sum to the whole corpus. A call that
 * returned at a harness ceiling is labelled, so a ranking cannot report the
 * harness as a slow suite. And the wait row and the waiting panel come from one
 * function, so the two surfaces cannot print different numbers.
 */

import { describe, expect, test } from "bun:test";
import {
  type DrillTarget,
  type FinishContext,
  makeCorpusTotals,
} from "../../src/services/stats/accumulate";
import {
  classifyJob,
  isBackgrounded,
  isCapped,
  makeTimeTable,
  segments,
} from "../../src/services/stats/detectors/timing";
import {
  declaredDelaySec,
  isWaiting,
  makeWaitPanel,
  waitShape,
} from "../../src/services/stats/detectors/waiting";
import { DEFAULT_ROW_LIMIT } from "../../src/services/stats/rank";
import { makeRedactor } from "../../src/services/stats/redact";
import type { BashFact, WaitShape } from "../../src/services/stats/schema";

const redact = makeRedactor({ enabled: true });

const WORDS = /\s+/;

const opts = { redact, limit: DEFAULT_ROW_LIMIT, sampleCap: 3, drill: null };

interface Line {
  readonly cmd: string;
  readonly durSec?: number;
  readonly sid?: string;
}

/** One bash fact, minted the way `extract.ts` mints it. */
const bashFact = (line: Line, seq: number): BashFact => {
  const { cmd } = line;
  const durSec = line.durSec ?? 1;
  const { job, present } = classifyJob(cmd);
  return {
    row: "bash",
    sid: line.sid ?? "s1",
    agent: "claude",
    project: "p1",
    kind: "main",
    parent: null,
    run: null,
    seq,
    toolUseId: null,
    tool: "Bash",
    ts: "2026-08-01T00:00:00.000Z",
    durSec,
    err: false,
    failed: false,
    failReason: null,
    detector: null,
    benign: null,
    sig: null,
    arg: null,
    outChars: 0,
    side: false,
    cmd,
    bin: cmd.split(WORDS)[0] ?? "",
    family: cmd,
    job,
    present,
    wait: waitShape(cmd),
    declaredDelaySec: declaredDelaySec(cmd),
    piped: cmd.includes("|"),
    bg: isBackgrounded(cmd),
    capped: isCapped(durSec),
  };
};

/** Fold lines through the corpus totals and both detectors, as the service does. */
const run = (lines: readonly Line[], drill: DrillTarget | null = null) => {
  const facts = lines.map(bashFact);
  const totals = makeCorpusTotals();
  const time = makeTimeTable({ ...opts, drill });
  const waiting = makeWaitPanel({ ...opts, drill });
  for (const fact of facts) {
    totals.add(fact);
    time.add(fact);
    waiting.add(fact);
  }
  const caveats: string[] = [];
  const ctx: FinishContext = {
    corpus: totals.finish(),
    window: { days: 60, fromIso: "", toIso: "" },
    redact,
    limit: DEFAULT_ROW_LIMIT,
    sampleCap: 3,
    caveat: (note) => {
      caveats.push(note);
    },
  };
  return {
    caveats,
    facts,
    panel: waiting.finish(ctx),
    rows: time.finish(ctx),
    timeHits: time.drill(ctx),
    waitHits: waiting.drill(ctx),
  };
};

const CORPUS: readonly Line[] = [
  { cmd: "bun run typecheck && bun test", durSec: 30 },
  { cmd: "bun test packages/core", durSec: 12 },
  { cmd: "tsc --noEmit", durSec: 8 },
  { cmd: "git status && ls -la", durSec: 0.4 },
  { cmd: 'grep -rn "foo" src | head -20', durSec: 0.2 },
  { cmd: "sleep 30; curl localhost:3000/health", durSec: 31, sid: "s2" },
  {
    cmd: 'until [ "$(curl -s -o /dev/null -w %{http_code} localhost:5173)" = 200 ]; do sleep 5; done',
    durSec: 600,
    sid: "s2",
  },
  { cmd: "python3 scripts/probe.py", durSec: 3 },
  { cmd: "cat package.json", durSec: 0.05 },
  { cmd: "sleep 1980 && echo APOLLO_WINDOW_OPEN &", durSec: 0.17, sid: "s3" },
];

describe("classifyJob", () => {
  test("a compound line lands in exactly one job, the highest-priority one", () => {
    const { job, present } = classifyJob("bun run typecheck && bun test");
    expect(job).toBe("test");
    expect([...present].sort()).toEqual(["test", "typecheck"]);
  });

  test("waiting outranks everything, so a poll loop is never a test run", () => {
    const cmd =
      "while ! grep -q PASS /tmp/test.log; do sleep 5; bun test; done";
    const { job, present } = classifyJob(cmd);
    expect(job).toBe("wait/poll");
    expect(present).toContain("test");
  });

  test("the charged job is always present, so co-present can never sit below runs", () => {
    for (const cmd of [
      "env -u GITHUB_TOKEN gh pr list",
      "wibble --frobnicate",
    ]) {
      const { job, present } = classifyJob(cmd);
      expect(present).toContain(job);
    }
  });

  test("a wrapper, a cd and a path prefix all reduce to the binary", () => {
    expect(
      segments("cd /tmp/x && ../../.venv/bin/python -c 'print(1)'")
    ).toEqual(["python -c 'print(1)'"]);
    expect(segments("timeout 5m sudo docker build .")).toEqual([
      "docker build .",
    ]);
  });
});

describe("harness ceilings", () => {
  test("the 600s cap and the 120s band are capped; a 100s run is not", () => {
    expect(isCapped(600)).toBe(true);
    expect(isCapped(590)).toBe(true);
    expect(isCapped(120)).toBe(true);
    expect(isCapped(118)).toBe(true);
    expect(isCapped(124)).toBe(true);
    expect(isCapped(100)).toBe(false);
    expect(isCapped(125)).toBe(false);
    expect(isCapped(null)).toBe(false);
  });

  test("a capped run is split out of its job's total, never out of the ranking", () => {
    const { rows, caveats } = run(CORPUS);
    const wait = rows.find((r) => r.key === "wait/poll");
    expect(wait?.cappedRuns).toBe(1);
    expect(wait?.cappedSec).toBe(600);
    // raw stays raw; the net total is one subtraction away
    expect(wait?.totalSec).toBeGreaterThan(wait?.cappedSec ?? 0);
    expect(caveats.some((c) => c.includes("harness ceiling"))).toBe(true);
  });
});

describe("exclusive attribution", () => {
  test("every line is charged once, so runs and shares sum to the corpus", () => {
    const { rows, facts } = run(CORPUS);
    const runs = rows.reduce((sum, r) => sum + r.runs, 0);
    const shares = rows.reduce((sum, r) => sum + r.share, 0);
    const seconds = rows.reduce((sum, r) => sum + r.totalSec, 0);
    expect(runs).toBe(facts.length);
    expect(shares).toBeCloseTo(1, 3);
    expect(seconds).toBeCloseTo(
      facts.reduce((sum, f) => sum + (f.durSec ?? 0), 0),
      2
    );
  });

  test("co-present counts sit beside the attributed ones, never instead", () => {
    const { rows } = run(CORPUS);
    const typecheck = rows.find((r) => r.key === "typecheck");
    // `bun run typecheck && bun test` is charged to tests and co-present here
    expect(typecheck?.runs).toBe(1);
    expect(typecheck?.coPresentRuns).toBe(2);
    for (const row of rows) {
      expect(row.coPresentRuns).toBeGreaterThanOrEqual(row.runs);
    }
  });

  test("rows rank by total seconds, not by the worst single call", () => {
    const { rows } = run(CORPUS);
    const totals = rows.map((r) => r.totalSec);
    expect([...totals].sort((a, b) => b - a)).toEqual(totals);
  });
});

describe("waitShape", () => {
  const waits: readonly [string, WaitShape][] = [
    ["until [ -f /tmp/ready ]; do sleep 5; done", "loop-poll"],
    ["for i in $(seq 1 60); do gh pr view 1; sleep 5; done", "loop-poll"],
    ['python3 -c "import time; time.sleep(560); print(1)"', "bare-delay"],
    ["perl -e 'select undef,undef,undef,10'", "bare-delay"],
    ["/bin/sleep 30; echo ok", "bare-delay"],
    [
      "bun run main.ts > /tmp/x.log 2>&1 & sleep 5; tail -12 /tmp/x.log",
      "bare-delay",
    ],
    ["gh run watch 123 --exit-status --interval 20", "watcher"],
    ["curl --retry 5 https://example.com", "curl-retry"],
  ];

  for (const [cmd, shape] of waits) {
    test(`waits: ${cmd.slice(0, 46)}`, () => {
      expect(waitShape(cmd)).toBe(shape as never);
    });
  }

  const notWaits: readonly string[] = [
    "f url1 & f url2 & wait",
    "pgrep -P 4481 | while read p; do ps -o command= -p $p; done",
    "for i in $(seq 1 12); do curl localhost; done",
    'curl "https://api.github.com/x?since=2022-01-01&until=2023-06-01"',
    '.venv/bin/python -c "for d in xs: r = get(d); time.sleep(1.2)"',
    "vitest --watch",
    'grep -iE "vite|watch" /tmp/log',
    "cat > run.sh <<'EOF'\nsleep 25\nEOF",
  ];

  for (const cmd of notWaits) {
    test(`does not wait: ${cmd.slice(0, 46)}`, () => {
      expect(waitShape(cmd)).toBeNull();
    });
  }

  test("a prose apostrophe in a comment cannot swallow the delay below it", () => {
    expect(isWaiting("# anon wouldn't see this\nsleep 30; echo done")).toBe(
      true
    );
  });

  test("a heredoc body authors a file, it does not wait", () => {
    expect(declaredDelaySec("cat > run.sh <<'EOF'\nsleep 25\nEOF")).toBe(0);
    expect(declaredDelaySec("sleep 30 && sleep 12")).toBe(42);
  });
});

describe("the waiting panel and the wait row", () => {
  test("both read the same calls, seconds and share from one function", () => {
    const { rows, panel } = run(CORPUS);
    const row = rows.find((r) => r.key === "wait/poll");
    expect(panel.calls).toBe(row?.runs ?? 0);
    expect(panel.totalSec).toBe(row?.totalSec ?? 0);
    expect(panel.share).toBe(row?.share ?? 0);
  });

  test("the shapes partition the panel", () => {
    const { panel } = run(CORPUS);
    expect(panel.byShape.reduce((sum, s) => sum + s.calls, 0)).toBe(
      panel.calls
    );
    expect(panel.byShape.map((s) => s.key).sort()).toEqual([
      "bare-delay",
      "curl-retry",
      "loop-poll",
      "watcher",
    ]);
  });

  test("a backgrounded wait declares more than it is charged, and never sums in", () => {
    const { panel } = run(CORPUS);
    expect(panel.bgRuns).toBe(1);
    expect(panel.bgSec).toBe(0.17);
    expect(panel.bgDeclaredSec).toBe(1980);
    expect(panel.totalSec).toBeLessThan(panel.bgDeclaredSec);
  });
});

describe("drill", () => {
  test("nothing is retained unless a key was asked for", () => {
    const { timeHits, waitHits } = run(CORPUS);
    expect(timeHits).toHaveLength(0);
    expect(waitHits).toHaveLength(0);
  });

  test("a job key returns its calls, longest first", () => {
    const { rows, timeHits } = run(CORPUS, { table: "time", key: "wait/poll" });
    expect(timeHits).toHaveLength(
      rows.find((r) => r.key === "wait/poll")?.runs ?? 0
    );
    expect(timeHits.map((h) => h.durSec)).toEqual([600, 31, 0.17]);
  });

  test("a wait key takes one shape, or every shape", () => {
    expect(
      run(CORPUS, { table: "wait", key: "loop-poll" }).waitHits
    ).toHaveLength(1);
    expect(run(CORPUS, { table: "wait", key: "all" }).waitHits).toHaveLength(3);
  });
});
