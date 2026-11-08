/** The TUI session detail summary shows the session cost next to tokens. Synthetic fixtures only. */
import { expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { App } from "../src/tui/app";
import type { TuiBridge } from "../src/tui/runtime";

const priced = {
  provider: "claude-code",
  sessionId: "c0571234",
  path: "/x",
  title: "Priced session",
  models: ["claude-sonnet-4-5", "mystery-model"],
  cwd: "/repo",
  events: [],
  turns: [{ requestId: "r1", model: "claude-sonnet-4-5", costUsd: 1.23 }],
  compactionTurns: [],
  subagents: [],
  budget: [],
  snapshots: [],
  onDiskContextFiles: [],
  contextWindow: 200_000,
  contextWindowInferred: false,
  peakContextTokens: 50_000,
  peakTurnIndex: 0,
  finalContextTokens: 40_000,
  totalOutputTokens: 1000,
  systemOverheadTokens: 2000,
  peakCacheReadTokens: 10_000,
  dumbZoneCrossTurn: -1,
  dumbZoneFraction: 0.4,
  dumbZoneTurns: 0,
  turnCount: 1,
  userMessageCount: 1,
  toolCallCount: 0,
  messageCount: 2,
  sizeBytes: 100,
  cost: {
    totalUsd: 1.23,
    source: "estimate",
    unpricedModels: ["mystery-model"],
  },
} as unknown as AnalyzedSession;

const bridge: TuiBridge = {
  serverUrl: "http://127.0.0.1:4321",
  run: ((build: (c: unknown) => unknown) => {
    const client = {
      sessions: {
        list: () => ({ tag: "list" }),
        analyze: () => ({ tag: "analyze" }),
      },
      watch: { poll: () => ({ tag: "watch" }) },
      capabilities: { list: () => ({ tag: "caps" }) },
    };
    const eff = build(client) as { tag: string };
    if (eff.tag === "watch") {
      return Promise.resolve({ memory: 0, sessions: 0 });
    }
    if (eff.tag === "caps") {
      return Promise.resolve([]);
    }
    if (eff.tag === "analyze") {
      return Promise.resolve(priced);
    }
    return Promise.resolve([
      {
        id: "a",
        path: "/a",
        agent: "claude",
        project: "p",
        messageCount: 2,
        sizeBytes: 100,
        title: "Priced session",
        costUsd: 1.23,
        unpricedModels: ["mystery-model"],
      },
    ]);
  }) as TuiBridge["run"],
};

test("session detail summary shows the estimated cost", async () => {
  const setup = await testRender(
    <App bridge={bridge} onQuit={() => undefined} />,
    { width: 140, height: 40 }
  );
  try {
    let frame = "";
    for (let i = 0; i < 40 && !frame.includes("System + tools"); i++) {
      await setup.renderOnce();
      frame = setup.captureCharFrame();
      await new Promise((r) => setTimeout(r, 30));
    }
    expect(frame).toContain("System + tools");
    expect(frame).toContain("Est. cost");
    expect(frame).toContain("~$1.23 · unpriced: mystery-model");
  } finally {
    setup.renderer.destroy();
  }
});
