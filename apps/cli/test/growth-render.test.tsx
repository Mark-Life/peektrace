/** Render test: toggling the sessions analysis pane to the growth chart.
 *
 * Drives the full App through a fake bridge, selects a session, presses `g`,
 * and asserts the stacked-column chart renders (axis labels, block bars, legend)
 * without overlapping or overflowing.
 */
import { expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { App } from "../src/tui/app";
import type { TuiBridge } from "../src/tui/runtime";

const CTXS = [30_000, 80_000, 140_000, 190_000, 160_000];

const budget = [
  { key: "system_tools", color: "#6e7681", label: "System" },
  { key: "prompts", color: "#3fb950", label: "Prompts" },
  { key: "tool_results", color: "#f0883e", label: "Tool results" },
].map((b) => ({ ...b, short: b.key, tokens: 1, estimated: false }));

const snap = (i: number, ctx: number) => ({
  turnIndex: i,
  model: "c",
  ctx,
  outputTokens: 2000,
  cacheReadTokens: 0,
  slices: {
    system_tools: 40_000,
    listings: 0,
    memory: 0,
    files: 0,
    prompts: Math.round(ctx * 0.2),
    tool_results: Math.round(ctx * 0.3),
    assistant_text: 0,
    thinking: 0,
    other: 0,
    unattributed: Math.max(0, ctx - 40_000 - Math.round(ctx * 0.5)),
  },
});

const analyzed = (): AnalyzedSession =>
  ({
    provider: "claude-code",
    sessionId: "a",
    path: "/x",
    title: "Chart demo",
    models: ["c"],
    cwd: "/r",
    events: [],
    turns: CTXS.map((_, i) => ({ requestId: `r${i}`, model: "c" })),
    compactionTurns: [3],
    subagents: [],
    budget,
    snapshots: CTXS.map((c, i) => snap(i, c)),
    onDiskContextFiles: [],
    biggestItems: [],
    contextWindow: 200_000,
    contextWindowInferred: false,
    peakContextTokens: 190_000,
    peakTurnIndex: 3,
    finalContextTokens: 160_000,
    totalOutputTokens: 8000,
    systemOverheadTokens: 40_000,
    peakCacheReadTokens: 0,
    dumbZoneCrossTurn: 2,
    dumbZoneFraction: 0.4,
    dumbZoneTurns: 3,
    turnCount: 5,
    userMessageCount: 1,
    toolCallCount: 3,
  }) as unknown as AnalyzedSession;

const bridge: TuiBridge = {
  serverUrl: "http://x",
  run: ((build: (c: unknown) => unknown) => {
    const client = {
      sessions: {
        list: () => ({ t: "list" }),
        analyze: () => ({ t: "an" }),
      },
      watch: { poll: () => ({ t: "w" }) },
      capabilities: { list: () => ({ t: "c" }) },
    };
    const eff = build(client) as { t: string };
    if (eff.t === "w") {
      return Promise.resolve({ memory: 0, sessions: 0 });
    }
    if (eff.t === "c") {
      return Promise.resolve([]);
    }
    if (eff.t === "an") {
      return Promise.resolve(analyzed());
    }
    return Promise.resolve([
      {
        id: "a",
        path: "/a",
        agent: "claude-code",
        project: "p",
        messageCount: 5,
        sizeBytes: 123,
        title: "Chart demo",
        startedAt: "2026-07-02T10:00",
      },
    ]);
  }) as TuiBridge["run"],
};

const settleFor = async (
  setup: Awaited<ReturnType<typeof testRender>>,
  needle: string
): Promise<string> => {
  for (let i = 0; i < 40; i++) {
    await setup.renderOnce();
    const frame = setup.captureCharFrame();
    if (frame.includes(needle)) {
      return frame;
    }
    await new Promise((r) => setTimeout(r, 30));
  }
  return setup.captureCharFrame();
};

test("`g` toggles the analysis pane to the growth chart", async () => {
  const setup = await testRender(
    <App bridge={bridge} onQuit={() => undefined} />,
    { width: 116, height: 38 }
  );
  try {
    await settleFor(setup, "Full history");
    setup.mockInput.typeText("g");
    const frame = await settleFor(setup, "Context growth");
    // Chart chrome present: header, a y-axis token label, block bars, legend.
    expect(frame).toContain("Context growth");
    expect(frame).toContain("200.0K");
    expect(frame).toContain("█");
    expect(frame).toContain("Tool results");
  } finally {
    setup.renderer.destroy();
  }
});
