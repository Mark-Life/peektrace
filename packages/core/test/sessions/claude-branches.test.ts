/** Claude transcripts are trees (`parentUuid`): rewinds leave dead branches.
 * Synthetic fixture only. */
import { describe, expect, test } from "bun:test";
import { parseClaudeSession } from "../../src/services/sessions/parse";

const line = (o: Record<string, unknown>) => JSON.stringify(o);

const usage = (ctx: number) => ({
  input_tokens: 1,
  cache_read_input_tokens: ctx,
  cache_creation_input_tokens: 0,
  output_tokens: 5,
});

const user = (uuid: string, parentUuid: string | null, content: unknown) =>
  line({
    type: "user",
    uuid,
    parentUuid,
    timestamp: "2026-01-01T00:00:00Z",
    message: { role: "user", content },
  });

const assistant = (o: {
  readonly uuid: string;
  readonly parentUuid: string | null;
  readonly requestId: string;
  readonly ctx: number;
  readonly content?: unknown[];
}) =>
  line({
    type: "assistant",
    uuid: o.uuid,
    parentUuid: o.parentUuid,
    requestId: o.requestId,
    timestamp: "2026-01-01T00:00:01Z",
    message: {
      model: "claude-opus-9",
      usage: usage(o.ctx),
      content: o.content ?? [{ type: "text", text: "ok" }],
    },
  });

const toolResult = (uuid: string, parentUuid: string, toolUseId: string) =>
  line({
    type: "user",
    uuid,
    parentUuid,
    sourceToolAssistantUUID: parentUuid,
    timestamp: "2026-01-01T00:00:02Z",
    message: {
      role: "user",
      content: [{ type: "tool_result", tool_use_id: toolUseId, content: "r" }],
    },
  });

const parse = (lines: string[]) =>
  parseClaudeSession({
    text: lines.join("\n"),
    path: "/synthetic/s.jsonl",
    sessionId: "s",
  });

describe("parseClaudeSession: rewinds", () => {
  test("drops the abandoned branch after a rewind", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      user("u2", "a1", "second"),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 900 }),
      // Rewind to a1, prompt again.
      user("u2b", "a1", "second, edited"),
      assistant({ uuid: "a3", parentUuid: "u2b", requestId: "r3", ctx: 200 }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r3"]);
  });

  test("keeps billed usage of the abandoned branch in offBranchTurns", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      user("u2", "a1", "second"),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 900 }),
      user("u2b", "a1", "second, edited"),
      assistant({ uuid: "a3", parentUuid: "u2b", requestId: "r3", ctx: 200 }),
    ]);
    expect(p.offBranchTurns?.map((t) => t.requestId)).toEqual(["r2"]);
    expect(p.offBranchTurns?.[0]?.cacheReadTokens).toBe(900);
    expect(p.offBranchTurns?.[0]?.eventIndexes).toEqual([]);
  });

  test("sets no offBranchTurns without a rewind", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
    ]);
    expect(p.offBranchTurns).toBeUndefined();
  });

  test("drops tool calls and results on the dead branch", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      user("u2", "a1", "second"),
      assistant({
        uuid: "a2",
        parentUuid: "u2",
        requestId: "r2",
        ctx: 900,
        content: [{ type: "tool_use", id: "T1", name: "Read", input: {} }],
      }),
      toolResult("t1", "a2", "T1"),
      assistant({ uuid: "a2b", parentUuid: "t1", requestId: "r2b", ctx: 950 }),
      user("u2b", "a1", "second, edited"),
      assistant({ uuid: "a3", parentUuid: "u2b", requestId: "r3", ctx: 200 }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r3"]);
    expect(p.events.filter((e) => e.kind === "tool-result")).toHaveLength(0);
    expect(p.events.filter((e) => e.kind === "tool-call")).toHaveLength(0);
  });

  test("a duplicated prompt row is not a fork", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      user("u2", "a1", "second"),
      user("u2", "a1", "second"),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 150 }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r2"]);
  });

  test("ignores sidechain prompts sharing a parent", () => {
    const side = (uuid: string) =>
      line({
        type: "user",
        uuid,
        parentUuid: "a1",
        isSidechain: true,
        timestamp: "2026-01-01T00:00:00Z",
        message: { role: "user", content: "side" },
      });
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      user("u2", "a1", "second"),
      side("s1"),
      side("s2"),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 150 }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r2"]);
  });

  test("dead rows still feed session meta", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      line({
        type: "user",
        uuid: "u2",
        parentUuid: "a1",
        cwd: "/synthetic/repo",
        timestamp: "2026-01-01T00:00:00Z",
        message: { role: "user", content: "second" },
      }),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 900 }),
      user("u2b", "a1", "second, edited"),
      line({
        type: "assistant",
        uuid: "a3",
        parentUuid: "u2b",
        requestId: "r3",
        timestamp: "2026-01-01T00:00:09Z",
        message: { model: "claude-opus-9", usage: usage(200), content: [] },
      }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r3"]);
    expect(p.cwd).toBe("/synthetic/repo");
    expect(p.endedAt).toBe("2026-01-01T00:00:09Z");
  });

  test("falls back to every row when the last row would be dead", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      user("u2", "a1", "second"),
      user("u2b", "a1", "second, edited"),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 150 }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r2"]);
  });
});

describe("parseClaudeSession: tree shapes that are not rewinds", () => {
  test("keeps every result of parallel tool calls", () => {
    // Each tool_result points at the assistant row holding its tool_use, so
    // the parentUuid walk from the leaf skips the sibling result `t1`.
    const p = parse([
      user("u1", null, "go"),
      assistant({
        uuid: "a1",
        parentUuid: "u1",
        requestId: "r1",
        ctx: 100,
        content: [{ type: "tool_use", id: "T1", name: "Read", input: {} }],
      }),
      assistant({
        uuid: "a1b",
        parentUuid: "a1",
        requestId: "r1",
        ctx: 100,
        content: [{ type: "tool_use", id: "T2", name: "Read", input: {} }],
      }),
      toolResult("t1", "a1", "T1"),
      toolResult("t2", "a1b", "T2"),
      assistant({ uuid: "a2", parentUuid: "t2", requestId: "r2", ctx: 150 }),
    ]);
    const results = p.events.filter((e) => e.kind === "tool-result");
    expect(results).toHaveLength(2);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r2"]);
  });

  test("keeps turns before a compaction (null parentUuid, logicalParentUuid)", () => {
    const p = parse([
      user("u1", null, "first"),
      assistant({ uuid: "a1", parentUuid: "u1", requestId: "r1", ctx: 100 }),
      line({
        type: "system",
        subtype: "compact_boundary",
        uuid: "c1",
        parentUuid: null,
        logicalParentUuid: "a1",
        timestamp: "2026-01-01T00:00:03Z",
        compactMetadata: { trigger: "auto", preTokens: 100 },
      }),
      user("u2", "c1", "after"),
      assistant({ uuid: "a2", parentUuid: "u2", requestId: "r2", ctx: 50 }),
    ]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["r1", "r2"]);
  });
});
