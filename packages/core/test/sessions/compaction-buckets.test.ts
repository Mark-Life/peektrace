/** Compaction rewrites history, so listings and memory restart from zero.
 *
 * Claude Code re-attaches skill / agent / tool / MCP listings right after the
 * compact summary. Counting the old copies too would double the slice.
 * Nested memory is not re-attached, so it drops out of context.
 */
import { describe, expect, test } from "bun:test";
import { analyze } from "../../src/services/sessions/analyze";
import { parseClaudeSession } from "../../src/services/sessions/parse";

const usage = (ctx: number) => ({
  input_tokens: 10,
  cache_read_input_tokens: ctx - 10,
  cache_creation_input_tokens: 0,
  output_tokens: 20,
});

const skills = "- demo-skill: does a demo thing\n".repeat(25);
const agents = ["- helper: a demo agent", "- checker: a demo agent"];
const tools = ["DemoToolA", "DemoToolB", "DemoToolC"];
const mcp = ["## demo-server\nUse the demo server for demo things."];
const memory = "# Demo rules\nKeep functions small.\n".repeat(10);

const listingLines = [
  { type: "skill_listing", content: skills, skillCount: 25 },
  { type: "agent_listing_delta", addedLines: agents },
  { type: "deferred_tools_delta", addedNames: tools },
  { type: "mcp_instructions_delta", addedBlocks: mcp },
];

const memoryLine = {
  type: "nested_memory",
  path: "/repo/sub/CLAUDE.md",
  content: { content: memory },
};

const attach = (a: object, n: number) => ({
  type: "attachment",
  uuid: `a${n}`,
  timestamp: "2026-11-01T09:00:00.000Z",
  attachment: a,
});

const assistant = (req: string, ctx: number) => ({
  type: "assistant",
  uuid: `t-${req}`,
  timestamp: "2026-11-01T09:00:05.000Z",
  requestId: req,
  message: {
    model: "claude-opus-5-5",
    content: [{ type: "text", text: "ok" }],
    usage: usage(ctx),
  },
});

const user = (text: string, extra: object = {}) => ({
  type: "user",
  uuid: `u-${text.length}`,
  timestamp: "2026-11-01T09:00:00.000Z",
  sessionId: "s1",
  message: { role: "user", content: text },
  ...extra,
});

const lines = [
  user("start the demo"),
  ...[...listingLines, memoryLine].map((a, i) => attach(a, i)),
  assistant("r1", 50_000),
  {
    type: "system",
    subtype: "compact_boundary",
    content: "Conversation compacted",
  },
  user("Summary of the demo so far.", { isCompactSummary: true }),
  ...listingLines.map((a, i) => attach(a, i + 10)),
  user("continue"),
  assistant("r2", 60_000),
];

const parsed = () =>
  parseClaudeSession({
    text: lines.map((l) => JSON.stringify(l)).join("\n"),
    path: "/demo/s1.jsonl",
    sessionId: "s1",
  });

describe("compaction resets re-attached buckets", () => {
  test("listings are counted once after a compaction", () => {
    const a = analyze(parsed());
    expect(a.compactionTurns).toEqual([1]);
    const [before, after] = a.snapshots;
    expect(before?.slices.listings).toBeGreaterThan(0);
    expect(after?.slices.listings).toBe(before?.slices.listings);
  });

  test("memory loaded before a compaction drops to zero", () => {
    const a = analyze(parsed());
    const [before, after] = a.snapshots;
    expect(before?.slices.memory).toBeGreaterThan(0);
    expect(after?.slices.memory).toBe(0);
  });
});
