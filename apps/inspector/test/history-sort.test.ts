/** Table sort: stored-value migration, header clicks, and row ordering. */
import { expect, test } from "bun:test";

import type { TimelineEvent } from "@workspace/view-core/api-types.gen";

import {
  DEFAULT_HISTORY_SORT,
  historySortOf,
  isForwardOrder,
  nextSort,
  sortRows,
} from "../src/lib/history-sort";

const row = (
  pos: number,
  over: Partial<TimelineEvent> = {}
): { e: TimelineEvent; pos: number } => ({
  e: {
    body: "",
    index: pos,
    kind: "assistant-text",
    preview: "",
    title: "",
    tokensEst: 0,
    ...over,
  },
  pos,
});

const ROWS = [
  row(0, { kind: "user-prompt", tokensEst: 10 }),
  row(1, { tokensEst: 300 }),
  row(2, { kind: "tool-call", tokensEst: 300, toolName: "Read" }),
  row(3, { tokensEst: 50 }),
];

const posOf = (rows: readonly { pos: number }[]) => rows.map((r) => r.pos);

test("old stored values migrate; junk falls back to transcript order", () => {
  expect(historySortOf("size")).toEqual({ dir: "desc", key: "tokens" });
  expect(historySortOf("order")).toEqual(DEFAULT_HISTORY_SORT);
  expect(historySortOf({ dir: "desc", key: "name" })).toEqual(
    DEFAULT_HISTORY_SORT
  );
  expect(historySortOf({ dir: "sideways", key: "type" })).toEqual({
    dir: "asc",
    key: "type",
  });
});

test("clicking the active column flips it; a new column starts natural", () => {
  expect(nextSort(DEFAULT_HISTORY_SORT, "order")).toEqual({
    dir: "desc",
    key: "order",
  });
  expect(nextSort(DEFAULT_HISTORY_SORT, "tokens")).toEqual({
    dir: "desc",
    key: "tokens",
  });
  expect(nextSort({ dir: "desc", key: "tokens" }, "type")).toEqual({
    dir: "asc",
    key: "type",
  });
});

test("rows sort by key, ties keep transcript order, pos is kept", () => {
  expect(posOf(sortRows(ROWS, DEFAULT_HISTORY_SORT))).toEqual([0, 1, 2, 3]);
  expect(posOf(sortRows(ROWS, { dir: "desc", key: "order" }))).toEqual([
    3, 2, 1, 0,
  ]);
  expect(posOf(sortRows(ROWS, { dir: "desc", key: "tokens" }))).toEqual([
    1, 2, 3, 0,
  ]);
  expect(posOf(sortRows(ROWS, { dir: "asc", key: "tokens" }))).toEqual([
    0, 3, 1, 2,
  ]);
  // Badge labels: "assistant-text" < "Read" < "user-prompt"; ties by pos.
  expect(posOf(sortRows(ROWS, { dir: "asc", key: "type" }))).toEqual([
    1, 3, 2, 0,
  ]);
});

test("sorting never mutates the input", () => {
  const input = [...ROWS];
  sortRows(input, { dir: "desc", key: "tokens" });
  expect(posOf(input)).toEqual([0, 1, 2, 3]);
});

test("only order ascending is forward order", () => {
  expect(isForwardOrder(DEFAULT_HISTORY_SORT)).toBe(true);
  expect(isForwardOrder({ dir: "desc", key: "order" })).toBe(false);
  expect(isForwardOrder({ dir: "asc", key: "tokens" })).toBe(false);
});
