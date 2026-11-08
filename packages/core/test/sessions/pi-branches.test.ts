/** Pi sessions are trees (`parentId`); the active branch runs from the last
 * entry back to the root. Synthetic fixture only. */
import { describe, expect, test } from "bun:test";
import { parsePiSession } from "../../src/services/sessions/parsers/pi";

const line = (o: Record<string, unknown>) => JSON.stringify(o);
const TS = "2026-01-01T00:00:00.000Z";

const user = (id: string, parentId: string | null | undefined, text: string) =>
  line({
    type: "message",
    id,
    ...(parentId === undefined ? {} : { parentId }),
    timestamp: TS,
    message: { role: "user", content: [{ type: "text", text }] },
  });

const assistant = (
  id: string,
  parentId: string | null | undefined,
  cacheRead: number
) =>
  line({
    type: "message",
    id,
    ...(parentId === undefined ? {} : { parentId }),
    timestamp: TS,
    message: {
      role: "assistant",
      content: [{ type: "text", text: "ok" }],
      model: "demo-model",
      usage: { input: 1, output: 2, cacheRead, cacheWrite: 0 },
    },
  });

const HEADER = line({
  type: "session",
  version: 3,
  id: "pi-synthetic",
  timestamp: TS,
  cwd: "/synthetic/proj",
});

const parse = (lines: string[]) =>
  parsePiSession({
    text: lines.join("\n"),
    path: "/synthetic/pi.jsonl",
    sessionId: "fallback",
    slug: "--synthetic-proj--",
  });

/** Request ids of turns on the timeline: abandoned-branch turns have no events. */
const live = (p: ReturnType<typeof parse>) =>
  p.turns.filter((t) => t.eventIndexes.length > 0).map((t) => t.requestId);

describe("parsePiSession: tree sessions", () => {
  test("follows the active branch from the leaf", () => {
    const p = parse([
      HEADER,
      user("u1", null, "first"),
      assistant("a1", "u1", 100),
      user("u2", "a1", "second"),
      assistant("a2", "u2", 900),
      // /tree back to a1, with a summary of the abandoned path.
      line({
        type: "branch_summary",
        id: "bs",
        parentId: "a1",
        fromId: "a2",
        timestamp: TS,
        summary: "tried something",
      }),
      user("u3", "bs", "second, again"),
      assistant("a3", "u3", 200),
    ]);
    expect(live(p)).toEqual(["a1", "a3"]);
  });

  test("keeps billed usage off the active branch in turns, with no events", () => {
    const p = parse([
      HEADER,
      user("u1", null, "first"),
      assistant("a1", "u1", 100),
      user("u2", "a1", "second"),
      assistant("a2", "u2", 900),
      user("u3", "a1", "second, again"),
      assistant("a3", "u3", 200),
    ]);
    expect(live(p)).toEqual(["a1", "a3"]);
    expect(p.turns.map((t) => t.requestId)).toEqual(["a1", "a2", "a3"]);
    const dead = p.turns.find((t) => t.requestId === "a2");
    expect(dead?.cacheReadTokens).toBe(900);
    expect(dead?.eventIndexes).toEqual([]);
    expect(p.events.some((e) => e.requestId === "a2")).toBe(false);
  });

  test("falls back to file order when entries have no parentId", () => {
    const p = parse([
      HEADER,
      user("u1", undefined, "first"),
      assistant("a1", undefined, 100),
      user("u2", undefined, "second"),
      assistant("a2", undefined, 200),
    ]);
    expect(live(p)).toEqual(["a1", "a2"]);
  });

  test("ignores a context_edit on the dead branch", () => {
    const p = parse([
      HEADER,
      user("u1", null, "first"),
      assistant("a1", "u1", 100),
      user("u2", "a1", "second"),
      line({
        type: "context_edit",
        id: "e1",
        parentId: "u2",
        targetId: "u1",
        replacement: null,
        timestamp: TS,
      }),
      user("u3", "a1", "second, again"),
      assistant("a3", "u3", 200),
    ]);
    const first = p.events.find((e) => e.index === 1);
    expect(first).toBeDefined();
    expect(first?.contextEdit).toBeUndefined();
    expect(live(p)).toEqual(["a1", "a3"]);
  });

  test("falls back to file order when a parent is missing", () => {
    const p = parse([
      HEADER,
      user("u1", null, "first"),
      assistant("a1", "u1", 100),
      user("u2", "a1", "second"),
      assistant("a2", "u2", 900),
      user("u3", "gone", "orphan"),
      assistant("a3", "u3", 200),
    ]);
    expect(live(p)).toEqual(["a1", "a2", "a3"]);
  });

  test("a session_info title off the branch still applies", () => {
    const p = parse([
      HEADER,
      user("u1", null, "first"),
      assistant("a1", "u1", 100),
      line({
        type: "session_info",
        id: "i1",
        parentId: "a1",
        name: "Synthetic title",
        timestamp: TS,
      }),
      user("u3", "a1", "second"),
      assistant("a3", "u3", 200),
    ]);
    expect(p.title).toBe("Synthetic title");
    expect(live(p)).toEqual(["a1", "a3"]);
  });
});
