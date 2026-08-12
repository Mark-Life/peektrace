/** What the stats screen must survive: a corpus with nothing in it, a terminal
 * smaller than the layout, counts wide enough to burst a column, and a
 * transcript carrying raw escape sequences.
 *
 * Each case is a way the screen can corrupt a terminal rather than fail loudly:
 * a share divided by zero, a row that wraps and shifts every row under it, a
 * control byte reaching the buffer. All four are pinned here on fixtures.
 */
import { expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { StatsReport } from "@workspace/core/services/stats/schema";
import { App } from "../src/tui/app";
import type { TuiBridge } from "../src/tui/runtime";

const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);

const WIDE = 100;
const TALL = 44;
const NARROW = 80;
const SHORT = 24;
const TINY = 60;

/** A window with no calls in it: every share has a zero denominator. */
const empty = (): StatsReport =>
  ({
    schemaVersion: "peektrace-stats/v1",
    generatedAt: Date.now(),
    window: { days: 60, fromIso: "a", toIso: "b" },
    detectorVersion: 1,
    corpus: {
      transcripts: 0,
      sessions: 0,
      projects: 0,
      toolCalls: 0,
      bashCalls: 0,
      bashSec: 0,
      toolSec: 0,
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
  }) as StatsReport;

/** Six-figure counts against three-figure hours: the widest cells that exist. */
const fatNumbers = (): StatsReport =>
  ({
    ...empty(),
    corpus: { ...empty().corpus, transcripts: 3, bashSec: 900 },
    fails: [
      {
        key: "k",
        label: "a failing thing",
        where: "Bash",
        rows: 3,
        sessions: 2,
        projects: 1,
        flagged: 1,
        note: "quote it",
        detector: "zsh",
        samples: [],
      },
    ],
    time: [
      {
        key: "test",
        label: "tests",
        runs: 123_456,
        coPresentRuns: 1,
        totalSec: 900_000,
        share: 0.5,
        medianSec: 1,
        p95Sec: 2,
        cappedRuns: 123_456,
        cappedSec: 900_000,
        bgRuns: 123_456,
        bgSec: 900_000,
        samples: [],
      },
    ],
  }) as StatsReport;

/** A hit whose every string carries an escape sequence or an oversized id. */
const hostileHit = {
  sid: `${ESC}[31mabcdefghijklmnopqrstuvwxyz0123456789`,
  agent: "claude",
  project: `${ESC}]0;title${BEL}/tmp/p`,
  kind: "main",
  seq: 1,
  line: `${ESC}[1;32mrm -rf / tail`,
  ts: `${ESC}[Anot-a-date-at-all`,
  durSec: 5,
  tool: `Ba${ESC}[0msh`,
  failed: true,
};

const makeBridge = (report: () => StatsReport): TuiBridge => ({
  serverUrl: "http://127.0.0.1:4321",
  run: ((build: (c: unknown) => unknown) => {
    const client = {
      sessions: { list: () => ({ tag: "list" }) },
      watch: { poll: () => ({ tag: "watch" }) },
      capabilities: { list: () => ({ tag: "caps" }) },
      stats: {
        report: () => ({ tag: "report" }),
        refresh: () => ({ tag: "refresh" }),
        drill: () => ({ tag: "drill" }),
      },
    };
    const eff = build(client) as { tag: string };
    if (eff.tag === "watch") {
      return Promise.resolve({ memory: 0, sessions: 1 });
    }
    if (eff.tag === "report") {
      return Promise.resolve(report());
    }
    if (eff.tag === "drill") {
      return Promise.resolve({
        schemaVersion: "peektrace-stats/v1",
        table: "fail",
        key: "k",
        total: 1,
        offset: 0,
        hits: [hostileHit],
      });
    }
    return Promise.resolve([]);
  }) as TuiBridge["run"],
});

const SETTLE_ROUNDS = 40;
const SETTLE_MS = 30;

/** Poll for `needle` while advancing the timer queue `waitForFrame` skips. */
const settleFor = async (
  setup: Awaited<ReturnType<typeof testRender>>,
  needle: string
): Promise<string> => {
  for (let i = 0; i < SETTLE_ROUNDS; i++) {
    await setup.renderOnce();
    const frame = setup.captureCharFrame();
    if (frame.includes(needle)) {
      return frame;
    }
    await new Promise((r) => setTimeout(r, SETTLE_MS));
  }
  return setup.captureCharFrame();
};

/** The longest rendered line: anything past the terminal width has wrapped. */
const widest = (frame: string): number =>
  Math.max(...frame.split("\n").map((line) => line.trimEnd().length));

const openStats = async (report: () => StatsReport, size: [number, number]) => {
  const setup = await testRender(
    <App bridge={makeBridge(report)} onQuit={() => undefined} />,
    { width: size[0], height: size[1] }
  );
  await setup.waitForFrame((v) => v.includes("loopback"));
  await setup.mockInput.typeText("5");
  return setup;
};

test("an empty corpus renders both tables and divides by nothing", async () => {
  const setup = await openStats(empty, [WIDE, TALL]);
  try {
    const frame = await settleFor(setup, "WHAT COSTS TIME");
    expect(frame).toContain("No failing calls");
    expect(frame).toContain("No shell seconds");
    expect(frame).not.toContain("NaN");
    expect(widest(frame)).toBeLessThanOrEqual(WIDE);
  } finally {
    setup.renderer.destroy();
  }
});

test("six-figure capped and backgrounded counts keep the row on one line", async () => {
  const setup = await openStats(fatNumbers, [WIDE, TALL]);
  try {
    const frame = await settleFor(setup, "WHAT COSTS TIME");
    const row = frame.split("\n").find((line) => line.includes("250h00m"));
    expect(row).toBeDefined();
    // The whole row on one line: job, time, share, runs, capped and bg.
    expect(row).toContain("123,456");
    expect(widest(frame)).toBeLessThanOrEqual(WIDE);
  } finally {
    setup.renderer.destroy();
  }
});

test("the narrow layout fits an 80x24 terminal without wrapping a row", async () => {
  const setup = await openStats(fatNumbers, [NARROW, SHORT]);
  try {
    const frame = await settleFor(setup, "WHAT FAILS");
    expect(frame).toContain("WHAT COSTS TIME");
    expect(widest(frame)).toBeLessThanOrEqual(NARROW);
  } finally {
    setup.renderer.destroy();
  }
});

test("a terminal below the layout's floor says so instead of scrambling", async () => {
  const setup = await openStats(fatNumbers, [TINY, SHORT]);
  try {
    const frame = await settleFor(setup, "too small");
    expect(frame).toContain("stats needs 80×24");
    expect(frame).not.toContain("WHAT FAILS");
  } finally {
    setup.renderer.destroy();
  }
});

test("no escape sequence from a transcript reaches the drill overlay", async () => {
  const setup = await openStats(fatNumbers, [WIDE, TALL]);
  try {
    await settleFor(setup, "WHAT FAILS");
    await setup.mockInput.typeText("\r");
    const frame = await settleFor(setup, "DRILL");
    expect(frame).not.toContain(ESC);
    expect(frame).not.toContain(BEL);
    expect(frame).toContain("rm -rf / tail");
    expect(widest(frame)).toBeLessThanOrEqual(WIDE);
  } finally {
    setup.renderer.destroy();
  }
});
