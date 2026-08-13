/** Render tests for the Stats screen.
 *
 * Drives the real screen through a fake bridge that counts `stats.report` calls
 * while the watch poller keeps bumping its session version — the screen must
 * redraw the staleness marker and never re-issue the corpus pass. Also pins the
 * two widths: at 100 columns the wide columns are on screen, at 80 they are
 * dropped rather than overflowing.
 */
import { expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { StatsReport } from "@workspace/core/services/stats/schema";
import { App } from "../src/tui/app";
import type { TuiBridge } from "../src/tui/runtime";

const report = (): StatsReport =>
  ({
    schemaVersion: "peektrace-stats/v1",
    generatedAt: Date.now(),
    window: { days: 30, fromIso: "a", toIso: "b" },
    detectorVersion: 1,
    corpus: {
      transcripts: 2509,
      sessions: 2491,
      projects: 26,
      toolCalls: 64_644,
      bashCalls: 36_324,
      bashSec: 85_680,
      toolSec: 90_000,
      humanWaitSec: 0,
      kinds: { main: 2509 },
      agents: { claude: 2509 },
    },
    fails: [
      {
        key: "detect:zsh-equals-abort",
        label: "unquoted echo === separator",
        where: "Bash",
        rows: 169,
        sessions: 90,
        projects: 11,
        flagged: 167,
        note: 'quote the separator: echo "===" — zsh expands a bare === to a command name',
        detector: "zsh-equals-abort",
        samples: [
          {
            sid: "019fd37c-1111",
            agent: "claude",
            project: "p",
            kind: "main",
            seq: 1,
            line: "echo ===",
          },
        ],
      },
      {
        key: "scan:tsc",
        label: "error TS####",
        where: "Bash exit 0",
        rows: 619,
        sessions: 217,
        projects: 11,
        flagged: 0,
        note: "591 of 619 pipe into head/tail/grep, so the exit code is the pipe's",
        detector: null,
        samples: [],
      },
    ],
    time: [
      {
        key: "test",
        label: "tests",
        runs: 1752,
        coPresentRuns: 1764,
        totalSec: 20_340,
        share: 0.238,
        medianSec: 1.7,
        p95Sec: 33.5,
        cappedRuns: 15,
        cappedSec: 6060,
        bgRuns: 10,
        bgSec: 21.4,
        samples: [],
      },
      {
        key: "wait/poll",
        label: "waiting",
        runs: 703,
        coPresentRuns: 703,
        totalSec: 18_060,
        share: 0.211,
        medianSec: 4.8,
        p95Sec: 90,
        cappedRuns: 18,
        cappedSec: 6480,
        bgRuns: 266,
        bgSec: 2384,
        samples: [],
      },
    ],
    waiting: {
      calls: 703,
      totalSec: 18_060,
      share: 0.211,
      sessions: 150,
      projects: 13,
      medianSec: 4.8,
      p95Sec: 90,
      byShape: [{ key: "loop-poll", calls: 217, totalSec: 12_900, share: 0.7 }],
      bgRuns: 266,
      bgSec: 2384,
      bgDeclaredSec: 15_300,
      samples: [],
    },
    clusters: [],
    context: [],
    fanout: [],
    web: [],
    cost: [],
    skipped: [],
    caveats: [
      "durSec is the gap between a call and its result: it includes harness overhead",
      "silent-failure counts are a floor: a detector only sees stored output",
    ],
  }) as StatsReport;

/** Fake bridge counting report passes, with a watch version that keeps moving. */
const makeBridge = () => {
  const calls = { report: 0, watch: 0 };
  const bridge: TuiBridge = {
    serverUrl: "http://127.0.0.1:4321",
    run: ((build: (c: unknown) => unknown) => {
      const client = {
        sessions: { list: () => ({ tag: "list" }) },
        watch: { poll: () => ({ tag: "watch" }) },
        capabilities: { list: () => ({ tag: "caps" }) },
        stats: {
          report: (p: { days?: number }) => ({ tag: "report", days: p.days }),
          refresh: () => ({ tag: "refresh" }),
          drill: () => ({ tag: "drill" }),
        },
      };
      const eff = build(client) as { tag: string; days?: number };
      if (eff.tag === "watch") {
        calls.watch += 1;
        return Promise.resolve({ memory: 0, sessions: calls.watch });
      }
      if (eff.tag === "report") {
        calls.report += 1;
        return Promise.resolve(report());
      }
      if (eff.tag === "drill") {
        return Promise.resolve({
          schemaVersion: "peektrace-stats/v1",
          table: "fail",
          key: "k",
          total: 0,
          offset: 0,
          hits: [],
        });
      }
      return Promise.resolve([]);
    }) as TuiBridge["run"],
  };
  return { bridge, calls };
};

/** Poll for `needle` while advancing the timer queue `waitForFrame` skips. */
const settleFor = async (
  setup: Awaited<ReturnType<typeof testRender>>,
  needle: string,
  rounds = 40
): Promise<string> => {
  for (let i = 0; i < rounds; i++) {
    await setup.renderOnce();
    const frame = setup.captureCharFrame();
    if (frame.includes(needle)) {
      return frame;
    }
    await new Promise((r) => setTimeout(r, 30));
  }
  return setup.captureCharFrame();
};

const widest = (frame: string): number =>
  Math.max(...frame.split("\n").map((line) => line.trimEnd().length));

test("both ranked tables render with their ranking keys at 100 columns", async () => {
  const { bridge } = makeBridge();
  const setup = await testRender(
    <App bridge={bridge} onQuit={() => undefined} />,
    {
      width: 100,
      height: 44,
    }
  );
  try {
    await setup.waitForFrame((v) => v.includes("Peektrace"));
    await setup.mockInput.typeText("5");
    const frame = await settleFor(setup, "WHAT COSTS TIME");
    expect(frame).toContain("FIX FIRST");
    expect(frame).toContain("ranked by sessions spanned");
    expect(frame).toContain("SESSIONS");
    expect(frame).toContain("TIME");
    expect(frame).toContain("unquoted echo === separator");
    expect(frame).toContain("waiting");
    // The corpus line every share divides by, and the drill route out.
    expect(frame).toContain("2,509 transcripts");
    expect(frame).toContain("NOTE");
    expect(widest(frame)).toBeLessThanOrEqual(100);
  } finally {
    setup.renderer.destroy();
  }
});

test("a watch bump marks the report stale and never rescans the corpus", async () => {
  const { bridge, calls } = makeBridge();
  const setup = await testRender(
    <App bridge={bridge} onQuit={() => undefined} />,
    {
      width: 100,
      height: 44,
    }
  );
  try {
    await setup.waitForFrame((v) => v.includes("Peektrace"));
    await setup.mockInput.typeText("5");
    await settleFor(setup, "WHAT COSTS TIME");
    const after = await settleFor(setup, "● stale", 120);
    expect(calls.watch).toBeGreaterThan(1);
    expect(calls.report).toBe(1);
    expect(after).toContain("● stale");
  } finally {
    setup.renderer.destroy();
  }
});

test("80 columns drops the wide columns instead of overflowing", async () => {
  const { bridge } = makeBridge();
  const setup = await testRender(
    <App bridge={bridge} onQuit={() => undefined} />,
    {
      width: 80,
      height: 42,
    }
  );
  try {
    // At 80 columns the shell header already sheds its labels, brand included.
    await setup.waitForFrame((v) => v.includes("loopback"));
    await setup.mockInput.typeText("5");
    const frame = await settleFor(setup, "WHAT COSTS TIME");
    expect(frame).toContain("WHAT FAILS");
    expect(frame).not.toContain("WHERE");
    expect(frame).not.toContain("MEDIAN");
    expect(widest(frame)).toBeLessThanOrEqual(80);
  } finally {
    setup.renderer.destroy();
  }
});
