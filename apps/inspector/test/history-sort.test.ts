/** Table sort: stored-value migration, header clicks, and row ordering. */
import { expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import {
  DEFAULT_HISTORY_SORT,
  historySortOf,
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
  row(2, { kind: "tool-call", toolName: "Read", tokensEst: 300 }),
  row(3, { tokensEst: 50 }),
];

const posOf = (rows: readonly { pos: number }[]) => rows.map((r) => r.pos);

test("old stored values migrate; junk falls back to transcript order", () => {
  expect(historySortOf("size")).toEqual({ key: "tokens", dir: "desc" });
  expect(historySortOf("order")).toEqual(DEFAULT_HISTORY_SORT);
  expect(historySortOf({ key: "name", dir: "desc" })).toEqual(
    DEFAULT_HISTORY_SORT
  );
  expect(historySortOf({ key: "type", dir: "sideways" })).toEqual({
    key: "type",
    dir: "asc",
  });
});

test("clicking the active column flips it; a new column starts natural", () => {
  expect(nextSort(DEFAULT_HISTORY_SORT, "order")).toEqual({
    key: "order",
    dir: "desc",
  });
  expect(nextSort(DEFAULT_HISTORY_SORT, "tokens")).toEqual({
    key: "tokens",
    dir: "desc",
  });
  expect(nextSort({ key: "tokens", dir: "desc" }, "type")).toEqual({
    key: "type",
    dir: "asc",
  });
});

test("rows sort by key, ties keep transcript order, pos is kept", () => {
  expect(posOf(sortRows(ROWS, DEFAULT_HISTORY_SORT))).toEqual([0, 1, 2, 3]);
  expect(posOf(sortRows(ROWS, { key: "order", dir: "desc" }))).toEqual([
    3, 2, 1, 0,
  ]);
  expect(posOf(sortRows(ROWS, { key: "tokens", dir: "desc" }))).toEqual([
    1, 2, 3, 0,
  ]);
  expect(posOf(sortRows(ROWS, { key: "tokens", dir: "asc" }))).toEqual([
    0, 3, 1, 2,
  ]);
  // Badge labels: "assistant-text" < "Read" < "user-prompt"; ties by pos.
  expect(posOf(sortRows(ROWS, { key: "type", dir: "asc" }))).toEqual([
    1, 3, 2, 0,
  ]);
});

test("sorting never mutates the input", () => {
  const input = [...ROWS];
  sortRows(input, { key: "tokens", dir: "desc" });
  expect(posOf(input)).toEqual([0, 1, 2, 3]);
});
