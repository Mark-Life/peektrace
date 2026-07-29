/** Render tests for the redesigned Sessions screen.
 *
 * Drives the history (expand → highlighted body, ANSI sanitized) standalone, and
 * the card list + selection→detail wiring through a fake bridge, using
 * `@opentui/react/test-utils`.
 */
import { expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { App } from "../src/tui/app";
import type { TuiBridge } from "../src/tui/runtime";
import { BridgeProvider } from "../src/tui/runtime";
import { SessionHistory } from "../src/tui/screens/sessions-history";

const ESC = String.fromCharCode(27);

/** Minimal analyzed session carrying two timeline events. */
const fakeAnalyzed = (title: string): AnalyzedSession =>
  ({
    provider: "claude-code",
    sessionId: "abcd1234",
    path: "/x",
    title,
    models: ["claude"],
    cwd: "/repo",
    events: [
      {
        index: 0,
        kind: "tool-call",
        requestId: "r1",
        title: "Bash",
        preview: `${ESC}[32mBash tool call${ESC}[0m`,
        body: '{"command":"ls -la /tmp"}',
        tokensEst: 10,
        toolName: "Bash",
      },
      {
        index: 1,
        kind: "tool-result",
        requestId: "r1",
        title: "result",
        preview: "total 8",
        body: "total 8\nfile.txt",
        tokensEst: 5,
      },
    ],
    turns: [{ requestId: "r1", model: "claude" }],
    compactionTurns: [],
    subagents: [],
    budget: [],
    snapshots: [],
    onDiskContextFiles: [],
    biggestItems: [],
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
    toolCallCount: 1,
  }) as unknown as AnalyzedSession;

test("history expands to a highlighted body and strips ANSI", async () => {
  const setup = await testRender(
    <BridgeProvider
      bridge={{ serverUrl: "http://x", run: () => Promise.resolve(null) }}
    >
      <SessionHistory
        focused
        onBack={() => undefined}
        redact
        s={fakeAnalyzed("t")}
      />
    </BridgeProvider>,
    { width: 80, height: 24 }
  );
  try {
    const collapsed = await setup.waitForFrame((v) => v.includes("Bash tool"));
    // Collapsed preview shown, ANSI escape stripped, body not yet visible.
    expect(collapsed.includes(ESC)).toBe(false);
    expect(collapsed).not.toContain("ls -la /tmp");
    // Enter expands the selected (first) item → decoded bash body appears.
    setup.mockInput.pressEnter();
    const expanded = await setup.waitForFrame((v) => v.includes("ls -la /tmp"));
    expect(expanded).toContain("ls -la /tmp");
  } finally {
    setup.renderer.destroy();
  }
});

/** Fake bridge: list → two headers, analyze → title-per-id, watch → zeros. */
const fakeBridge: TuiBridge = {
  serverUrl: "http://127.0.0.1:4321",
  run: ((build: (c: unknown) => unknown) => {
    const client = {
      sessions: {
        list: () => ({ tag: "list" }),
        analyze: (p: { id: string }) => ({ tag: "analyze", id: p.id }),
      },
      watch: { poll: () => ({ tag: "watch" }) },
      capabilities: { list: () => ({ tag: "caps" }) },
    };
    const eff = build(client) as { tag: string; id?: string };
    if (eff.tag === "watch") {
      return Promise.resolve({ memory: 0, sessions: 0 });
    }
    if (eff.tag === "caps") {
      return Promise.resolve([]);
    }
    if (eff.tag === "analyze") {
      return Promise.resolve(fakeAnalyzed(`Analyzed-${eff.id}`));
    }
    return Promise.resolve([
      {
        id: "a",
        path: "/a",
        agent: "claude",
        project: "p",
        messageCount: 3,
        sizeBytes: 100,
        title: "Alpha",
        startedAt: "2026-07-02T10:00",
      },
      {
        id: "b",
        path: "/b",
        agent: "codex",
        project: "p",
        messageCount: 5,
        sizeBytes: 200,
        title: "Beta",
        startedAt: "2026-07-01T09:00",
      },
    ]);
  }) as TuiBridge["run"],
};

/**
 * Poll for `needle` while advancing the timer queue — the two-hop
 * list→analyze async chain needs real macrotasks that `waitForFrame` skips.
 */
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

test("session cards render and keyboard selection drives the detail", async () => {
  const setup = await testRender(
    <App bridge={fakeBridge} onQuit={() => undefined} />,
    { width: 120, height: 34 }
  );
  try {
    // Cards render (list resolved), then the detail analyzes the selection.
    const first = await settleFor(setup, "Analyzed-a");
    expect(first).toContain("Alpha");
    expect(first).toContain("Beta");
    expect(first).toContain("Analyzed-a");
    // Arrow-down selects Beta → the detail re-analyzes the new id.
    setup.mockInput.pressArrow("down");
    const second = await settleFor(setup, "Analyzed-b");
    expect(second).toContain("Analyzed-b");
  } finally {
    setup.renderer.destroy();
  }
});
