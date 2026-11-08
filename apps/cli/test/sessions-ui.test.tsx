/** Render tests for the redesigned Sessions screen.
 *
 * Drives the history (expand → highlighted body, ANSI sanitized) standalone, and
 * the card list + selection→detail wiring through a fake bridge, using
 * `@opentui/react/test-utils`.
 */
import { expect, setDefaultTimeout, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { act } from "react";
import { App } from "../src/tui/app";
import type { TuiBridge } from "../src/tui/runtime";
import { BridgeProvider } from "../src/tui/runtime";
import { SessionHistory } from "../src/tui/screens/sessions-history";
import { SETTLE_TEST_TIMEOUT_MS, settleFor } from "./support/settle";

setDefaultTimeout(SETTLE_TEST_TIMEOUT_MS);

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
    // Key presses become React updates that commit in a later macrotask, so
    // `act` flushes them before `waitForFrame` checks the frame.
    await act(() => {
      setup.mockInput.pressEnter();
    });
    const expanded = await setup.waitForFrame((v) => v.includes("ls -la /tmp"));
    expect(expanded).toContain("ls -la /tmp");
  } finally {
    setup.renderer.destroy();
  }
});

/** Same shape, but the last event is the biggest — so size order flips the rows. */
const fakeHeavyTail = (): AnalyzedSession => {
  const base = fakeAnalyzed("heavy tail");
  return {
    ...base,
    events: [base.events[0], { ...base.events[1], tokensEst: 500 }],
  } as unknown as AnalyzedSession;
};

test("s sorts the history biggest first and keeps open rows open", async () => {
  const setup = await testRender(
    <BridgeProvider
      bridge={{ serverUrl: "http://x", run: () => Promise.resolve(null) }}
    >
      <SessionHistory
        focused
        onBack={() => undefined}
        redact
        s={fakeHeavyTail()}
      />
    </BridgeProvider>,
    { width: 80, height: 24 }
  );
  try {
    const ordered = await setup.waitForFrame((v) => v.includes("Bash tool"));
    expect(ordered.indexOf("Bash tool")).toBeLessThan(
      ordered.indexOf("total 8")
    );
    // Expand the heavy result row.
    await act(() => {
      setup.mockInput.pressArrow("down");
    });
    await act(() => {
      setup.mockInput.pressEnter();
    });
    await setup.waitForFrame((v) => v.includes("file.txt"));
    // Size order lifts it above the smaller call, still open, with its share.
    await act(() => setup.mockInput.typeText("s"));
    const sorted = await setup.waitForFrame((v) => v.includes("~500   0.3%"));
    expect(sorted.indexOf("total 8")).toBeLessThan(sorted.indexOf("Bash tool"));
    expect(sorted).toContain("file.txt");
  } finally {
    setup.renderer.destroy();
  }
});

/** Same shape, but the timeline is the two attachments a badge must tell apart. */
const fakeAttachments = (): AnalyzedSession => {
  const base = fakeAnalyzed("attachments");
  const attachment = (index: number, attachmentType: string, preview: string) =>
    ({
      index,
      kind: "attachment",
      requestId: "r1",
      title: attachmentType,
      preview,
      body: preview,
      tokensEst: 8,
      ...(attachmentType === "" ? {} : { attachmentType }),
    }) as AnalyzedSession["events"][number];
  return {
    ...base,
    events: [
      attachment(0, "skill_listing", "- agent-to-human: Use when …"),
      attachment(1, "opened_file_in_ide", "drawer.tsx"),
      attachment(2, "", "no type at all"),
    ],
  };
};

test("attachment rows badge their type, not the bare kind", async () => {
  const setup = await testRender(
    <BridgeProvider
      bridge={{ serverUrl: "http://x", run: () => Promise.resolve(null) }}
    >
      <SessionHistory
        focused
        onBack={() => undefined}
        redact
        s={fakeAttachments()}
      />
    </BridgeProvider>,
    { width: 100, height: 24 }
  );
  try {
    const frame = await setup.waitForFrame((v) => v.includes("skill listing"));
    // `BADGE_MAX` is 20, so the longest known type renders whole.
    expect(frame).toContain("opened file in ide");
    // Typeless attachments keep the old fallback.
    expect(frame).toContain("attachment");
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
    // Header rows stay intact (regression: fixed rows must not collapse and
    // overlap when the card scrollbox grows).
    expect(first).toContain("filter (press /)");
    expect(first).toContain("agent: all · a to cycle");
    // Arrow-down selects Beta → the detail re-analyzes the new id.
    await act(() => {
      setup.mockInput.pressArrow("down");
    });
    await settleFor(setup, "Analyzed-b");
  } finally {
    setup.renderer.destroy();
  }
});
