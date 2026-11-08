/** The session verdict header shows the session cost as readable text. Synthetic fixtures only. */
import { expect, test } from "bun:test";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { renderToStaticMarkup } from "react-dom/server";
import { VerdictHeader } from "../src/components/verdict-header";

const base = {
  provider: "claude-code",
  sessionId: "c0571234",
  path: "/x",
  title: "Priced session",
  models: ["claude-sonnet-4-5", "mystery-model"],
  cwd: "/repo",
  events: [],
  turns: [],
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
} as unknown as AnalyzedSession;

const render = (a: AnalyzedSession) =>
  renderToStaticMarkup(<VerdictHeader a={a} />);

test("estimate cost renders as text with the unpriced models named", () => {
  const html = render({
    ...base,
    cost: {
      totalUsd: 1.23,
      source: "estimate",
      unpricedModels: ["mystery-model"],
    },
  });
  expect(html).toContain("Est. cost");
  expect(html).toContain("~$1.23");
  expect(html).toContain("unpriced: mystery-model");
  expect(html).toContain('title="Estimate at list price"');
});

test("agent-reported OpenCode cost is labelled as the agent's figure", () => {
  const html = render({
    ...base,
    provider: "opencode",
    cost: { totalUsd: 0.5, source: "agent" },
  });
  expect(html).toContain("~$0.50");
  expect(html).toContain("reported by OpenCode");
  expect(html).not.toContain("list price");
});

test("subagents share and unreadable subagents show as notes", () => {
  const html = render({
    ...base,
    cost: {
      totalUsd: 2,
      subagentsUsd: 0.75,
      source: "estimate",
      unreadableSubagents: 1,
    },
  });
  expect(html).toContain("subagents ~$0.75");
  expect(html).toContain("1 subagent(s) unreadable");
});

test("no cost means no cost cell", () => {
  expect(render(base)).not.toContain("Est. cost");
});

test("a fully unpriced session shows a dash, never $0.00", () => {
  const html = render({
    ...base,
    cost: { source: "estimate", unpricedModels: ["mystery-model"] },
  });
  expect(html).toContain("Est. cost");
  expect(html).toContain("—");
  expect(html).not.toContain("$0.00");
});
