/** Paging the history: which bodies drawn rows need, how they are fetched
 *  and stored, and index rows turned back into whole events. */
import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { GrowthSeries } from "../src/lib/api-types.gen";
import {
  buildChatPlan,
  type ChatNode,
  type EventRow,
} from "../src/lib/chat-lane";
import {
  type BodyCache,
  EMPTY_BODIES,
  hydrator,
  mergeRow,
  missingFetches,
  type Needed,
  neededBodies,
  pageOf,
  pagesFor,
  positionsOf,
  snapshotsOf,
  storeBodies,
  trimBodies,
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

const pages = (size: number, held: readonly number[]): BodyCache => ({
  pages: new Map(
    held.map((p) => [
      p,
      { events: EVENTS.slice(p * size, (p + 1) * size), hash: `h${p}` },
    ])
  ),
  rows: new Map(),
});

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

const pagesOf = (drawn: readonly number[], total: number, prefetch?: number) =>
  neededBodies({ drawn, prefetch, total }).pages;

describe("neededBodies", () => {
  test("rows in event order need their pages plus one page each way", () => {
    expect(pagesOf([250, 260], 1000)).toEqual([1, 2, 3]);
    const screen = Array.from({ length: 201 }, (_, i) => 250 + i);
    expect(pagesOf(screen, 1000, 2)).toEqual([0, 1, 2, 3, 4, 5, 6]);
  });

  test("clamped to the pages the session has", () => {
    expect(pagesOf([0, 5], 150)).toEqual([0, 1]);
    expect(pagesOf([120], 150)).toEqual([0, 1]);
    expect(pagesOf([0], 50)).toEqual([0]);
    expect(neededBodies({ drawn: [900], total: 150 })).toEqual({
      pages: [],
      rows: [],
    });
    expect(neededBodies({ drawn: [], total: 150 })).toEqual({
      pages: [],
      rows: [],
    });
  });

  test("a short list reports every row, so it needs every page", () => {
    const all = Array.from({ length: 150 }, (_, i) => i * 3);
    expect(pagesOf(all, 450)).toEqual([0, 1, 2, 3, 4]);
  });

  test("rows spread over the transcript need only their own events", () => {
    // A size sort: the biggest rows sit far apart.
    const drawn = [1500, 12, 640, 1720, 980, 13, 1100];
    expect(neededBodies({ drawn, total: 1861 })).toEqual({
      pages: [],
      rows: [12, 13, 640, 980, 1100, 1500, 1720],
    });
  });
});

describe("missingFetches", () => {
  const plan = (
    needed: Needed,
    {
      bodyLen = () => 10_000,
      cache = EMPTY_BODIES,
      skip = () => false,
    }: {
      readonly bodyLen?: (pos: number) => number;
      readonly cache?: BodyCache;
      readonly skip?: (n: number) => boolean;
    } = {}
  ) =>
    missingFetches({
      bodyLen,
      cache,
      hashes: ["h0", "h1", "h2"],
      needed,
      rev: "r2",
      size: 3,
      skipPage: skip,
      skipRow: skip,
    });

  test("pages not held at the summary's hash", () => {
    const cache = pages(3, [0, 1]);
    const held = {
      ...cache,
      pages: new Map(cache.pages).set(1, { events: [], hash: "old" }),
    };
    expect(plan({ pages: [0, 1, 2], rows: [] }, { cache: held })).toEqual([
      { hash: "h1", kind: "page", page: 1 },
      { hash: "h2", kind: "page", page: 2 },
    ]);
    expect(plan({ pages: [1, 2], rows: [] }, { skip: (p) => p === 1 })).toEqual(
      [{ hash: "h2", kind: "page", page: 2 }]
    );
  });

  test("spread rows fetch runs; cheap gaps join, big ones split", () => {
    const needed = { pages: [], rows: [0, 2, 6] };
    expect(plan(needed)).toEqual([
      { kind: "run", limit: 1, offset: 0, rev: "r2" },
      { kind: "run", limit: 1, offset: 2, rev: "r2" },
      { kind: "run", limit: 1, offset: 6, rev: "r2" },
    ]);
    expect(plan(needed, { bodyLen: (p) => (p === 1 ? 10 : 10_000) })).toEqual([
      { kind: "run", limit: 3, offset: 0, rev: "r2" },
      { kind: "run", limit: 1, offset: 6, rev: "r2" },
    ]);
  });

  test("rows held at the rev, in a current page or skipped are not asked", () => {
    const cache: BodyCache = {
      pages: pages(3, [1]).pages,
      rows: new Map([
        [0, { event: EVENTS[0] as TimelineEvent, rev: "r2" }],
        [6, { event: EVENTS[6] as TimelineEvent, rev: "r1" }],
      ]),
    };
    expect(
      plan({ pages: [], rows: [0, 4, 6, 7] }, { cache, skip: (p) => p === 7 })
    ).toEqual([{ kind: "run", limit: 1, offset: 6, rev: "r2" }]);
  });
});

describe("storeBodies and trimBodies", () => {
  const answer = (events: readonly TimelineEvent[], rev = "r") => ({
    events,
    hash: "h",
    rev,
  });

  test("a run stores its rows; a page drops the rows it holds", () => {
    const run = storeBodies(
      EMPTY_BODIES,
      { kind: "run", limit: 2, offset: 4, rev: "r" },
      answer(EVENTS.slice(4, 6)),
      3
    );
    expect(mergeRow(4, run, 3)).toBe(EVENTS[4] ?? null);
    expect(mergeRow(5, run, 3)).toBe(EVENTS[5] ?? null);
    expect(mergeRow(3, run, 3)).toBeNull();
    const page = storeBodies(
      run,
      { hash: "h", kind: "page", page: 1 },
      answer(EVENTS.slice(3, 6)),
      3
    );
    expect([...page.rows.keys()]).toEqual([]);
    expect(mergeRow(3, page, 3)).toBe(EVENTS[3] ?? null);
  });

  test("a row fetched after its page wins", () => {
    const newer = evt({ body: "newer" });
    const both = storeBodies(
      pages(3, [0]),
      { kind: "run", limit: 1, offset: 1, rev: "r" },
      answer([newer]),
      3
    );
    expect(mergeRow(1, both, 3)).toBe(newer);
    expect(mergeRow(0, both, 3)).toBe(EVENTS[0] ?? null);
  });

  test("a shrunk session drops bodies past its pages", () => {
    const cache = storeBodies(
      pages(3, [0, 1, 2]),
      { kind: "run", limit: 1, offset: 6, rev: "r" },
      answer([evt()]),
      3
    );
    const trimmed = trimBodies(cache, 2, 3);
    expect([...trimmed.pages.keys()]).toEqual([0, 1]);
    expect([...trimmed.rows.keys()]).toEqual([]);
    expect(trimBodies(trimmed, 2, 3)).toBe(trimmed);
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
});
