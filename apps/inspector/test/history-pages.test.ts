/** Paging the history: which pages drawn rows need, which held pages a new
 *  summary made stale, and index rows turned back into whole events. */
import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { GrowthSeries } from "../src/lib/api-types.gen";
import {
  buildChatPlan,
  type ChatNode,
  type EventRow,
} from "../src/lib/chat-lane";
import {
  hydrator,
  mergeRow,
  neededPages,
  type PageCache,
  pageOf,
  pagesFor,
  positionsOf,
  snapshotsOf,
  stalePages,
  turnRuleOf,
} from "../src/lib/history-pages";
import { indexRowsOf } from "./history-rows";

const evt = (over: Partial<TimelineEvent> = {}): TimelineEvent => ({
  body: "",
  index: 0,
  kind: "assistant-text",
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const EVENTS: readonly TimelineEvent[] = [
  evt({ body: "hi", kind: "user-prompt" }),
  evt({ body: "ok" }),
  evt({ body: "{}", kind: "tool-call", toolName: "Read", toolUseId: "t1" }),
  evt({ body: "file", kind: "tool-result", toolUseId: "t1" }),
  evt({ body: "rules", kind: "attachment", title: "file: A.md" }),
  evt({ body: "rules", kind: "attachment", title: "file: A.md" }),
  evt({ body: "bye" }),
];

const ROWS = indexRowsOf(EVENTS, [1, 1, 1, 1, 1, 1, 2]);

const pages = (size: number, held: readonly number[]): PageCache =>
  new Map(
    held.map((p) => [
      p,
      { events: EVENTS.slice(p * size, (p + 1) * size), hash: `h${p}` },
    ])
  );

describe("pageOf and pagesFor", () => {
  test("positions map to fixed pages", () => {
    expect(pageOf(0)).toBe(0);
    expect(pageOf(99)).toBe(0);
    expect(pageOf(100)).toBe(1);
    expect(pageOf(5, 2)).toBe(2);
  });

  test("pages are sorted and unique", () => {
    expect(pagesFor([250, 3, 99, 101, 250])).toEqual([0, 1, 2]);
    expect(pagesFor([])).toEqual([]);
  });
});

describe("positionsOf", () => {
  const plan = buildChatPlan({
    crossEvtIdx: 1,
    rows: ROWS.map((e, pos) => ({ e, pos })),
    turns: ROWS.map((r) => r.turn),
  });
  const byType = (type: ChatNode["type"]) =>
    plan.filter((n) => n.type === type).map(positionsOf);

  test("every node type names the events it draws", () => {
    expect(byType("message")).toEqual([[0], [1], [6]]);
    expect(byType("tool")).toEqual([[2, 3]]);
    expect(byType("run")).toEqual([[4, 5]]);
    expect(byType("turn")).toEqual([[], []]);
    expect(byType("dumbzone")).toEqual([[]]);
  });

  test("a table row is its own position", () => {
    expect(positionsOf({ e: ROWS[3] as EventRow, pos: 3 })).toEqual([3]);
  });

  test("a tool card with no call is its result", () => {
    const n: ChatNode<EventRow> = {
      call: null,
      result: { e: EVENTS[3] as TimelineEvent, pos: 3 },
      type: "tool",
    };
    expect(positionsOf(n)).toEqual([3]);
  });
});

describe("neededPages", () => {
  test("drawn pages plus one page each way", () => {
    expect(neededPages({ drawn: [250, 260], total: 1000 })).toEqual([1, 2, 3]);
    expect(
      neededPages({ drawn: [250, 450], prefetch: 2, total: 1000 })
    ).toEqual([0, 1, 2, 4, 5, 6]);
  });

  test("clamped to the pages the session has", () => {
    expect(neededPages({ drawn: [0, 5], total: 150 })).toEqual([0, 1]);
    expect(neededPages({ drawn: [120], total: 150 })).toEqual([0, 1]);
    expect(neededPages({ drawn: [0], total: 50 })).toEqual([0]);
    expect(neededPages({ drawn: [900], total: 150 })).toEqual([]);
    expect(neededPages({ drawn: [], total: 150 })).toEqual([]);
  });

  test("a short list reports every row, so it needs every page", () => {
    const all = Array.from({ length: 150 }, (_, i) => i * 3);
    expect(neededPages({ drawn: all, total: 450 })).toEqual([0, 1, 2, 3, 4]);
  });
});

describe("stalePages", () => {
  test("changed and new pages are stale", () => {
    expect(stalePages(["a", "b", "c"], ["a", "x", "c", "d"])).toEqual([1, 3]);
  });

  test("a shrunk session keeps only pages it still has", () => {
    expect(stalePages(["a", "b", "c"], ["a", "y"])).toEqual([1]);
    expect(stalePages([], [])).toEqual([]);
  });
});

describe("mergeRow and hydrator", () => {
  test("a row's event comes from its page, else null", () => {
    const held = pages(2, [1]);
    expect(mergeRow(2, held, 2)).toBe(EVENTS[2] ?? null);
    expect(mergeRow(3, held, 2)).toBe(EVENTS[3] ?? null);
    expect(mergeRow(0, held, 2)).toBeNull();
  });

  test("a node hydrates once every page it needs is held", () => {
    const plan = buildChatPlan({
      crossEvtIdx: -1,
      rows: ROWS.map((e, pos) => ({ e, pos })),
      turns: ROWS.map(() => 0),
    });
    const tool = plan.find((n) => n.type === "tool");
    if (!tool) {
      throw new Error("no tool node");
    }
    const half = pages(3, [0]);
    const partial = hydrator((pos) => mergeRow(pos, half, 3));
    expect(partial.node(tool)).toBeNull();
    const full = pages(3, [0, 1]);
    const h = hydrator((pos) => mergeRow(pos, full, 3));
    const whole = h.node(tool);
    expect(whole).toEqual({
      call: { e: EVENTS[2] as TimelineEvent, pos: 2 },
      result: { e: EVENTS[3] as TimelineEvent, pos: 3 },
      type: "tool",
    });
    // Same events, same node: memoized views see no change.
    expect(h.node(tool)).toBe(whole);
  });

  test("a whole event needs no page", () => {
    const h = hydrator(undefined);
    const row = { e: EVENTS[1] as TimelineEvent, pos: 1 };
    expect(h.row(row)).toEqual(row);
    expect(h.row({ e: ROWS[1] as EventRow, pos: 1 })).toBeNull();
  });
});

const GROWTH: GrowthSeries = {
  hash: "g",
  rev: "r",
  snapshots: {
    ctx: [10, 20],
    outputTokens: [1, 2],
    slices: {
      assistant_text: [1, 2],
      files: [0, 0],
      listings: [3, 3],
      memory: [0, 1],
      other: [0, 0],
      prompts: [2, 2],
      system_tools: [4, 4],
      thinking: [0, 5],
      tool_results: [0, 3],
      unattributed: [0, 0],
    },
    turnIndex: [0, 1],
  },
  turns: [
    { contextTokens: 10, model: "m", outputTokens: 1 },
    { contextTokens: 20, effort: "high", model: "m", outputTokens: 2 },
  ],
};

describe("growth", () => {
  test("columns become the snapshot rows the chart draws", () => {
    expect(snapshotsOf(GROWTH)).toEqual([
      {
        ctx: 10,
        outputTokens: 1,
        slices: {
          assistant_text: 1,
          files: 0,
          listings: 3,
          memory: 0,
          other: 0,
          prompts: 2,
          system_tools: 4,
          thinking: 0,
          tool_results: 0,
          unattributed: 0,
        },
        turnIndex: 0,
      },
      {
        ctx: 20,
        outputTokens: 2,
        slices: {
          assistant_text: 2,
          files: 0,
          listings: 3,
          memory: 1,
          other: 0,
          prompts: 2,
          system_tools: 4,
          thinking: 5,
          tool_results: 3,
          unattributed: 0,
        },
        turnIndex: 1,
      },
    ]);
  });

  test("turn rules read turn n at n - 1", () => {
    expect(turnRuleOf(GROWTH, 2)?.effort).toBe("high");
    expect(turnRuleOf(GROWTH, 3)).toBeUndefined();
  });
});
