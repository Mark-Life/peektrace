import { expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { RunItem, TranscriptRowRef } from "../src/lib/chat-lane";
import {
  chipWindow,
  foldRepeats,
  segmentsOf,
} from "../src/routes/sessions/chat-context-run";

const row = (pos: number) =>
  ({ e: {} as TimelineEvent, pos }) as TranscriptRowRef;

/** A run's items in order, with `open` positions split out as cards. */
const shape = (open: number[]) =>
  segmentsOf([0, 1, 2, 3, 4, 5].map(row), (r) => open.includes(r.pos)).map(
    (s) =>
      s.type === "card"
        ? `[${s.item.pos}]`
        : s.items.map((r) => r.pos).join(",")
  );

test("a closed run is one row of chips", () => {
  expect(shape([])).toEqual(["0,1,2,3,4,5"]);
});

test("an opened chip splits the run around it, keeping the order", () => {
  expect(shape([3])).toEqual(["0,1,2", "[3]", "4,5"]);
  expect(shape([1, 3])).toEqual(["0", "[1]", "2", "[3]", "4,5"]);
});

test("adjacent and edge cards leave no empty chip rows", () => {
  expect(shape([0, 1])).toEqual(["[0]", "[1]", "2,3,4,5"]);
  expect(shape([5])).toEqual(["0,1,2,3,4", "[5]"]);
});

const item = (pos: number, repeat: boolean) =>
  ({ e: {} as TimelineEvent, pos, repeat }) as RunItem;

const fold = (over: Partial<Parameters<typeof foldRepeats>[0]> = {}) =>
  foldRepeats({
    allOpen: false,
    forceExpand: false,
    items: [item(0, false), item(1, true), item(2, true)],
    opened: () => false,
    showRepeats: false,
    ...over,
  });

test("repeats fold into a count and leave fresh chips in order", () => {
  const { folded, visible } = fold();
  expect(visible.map((r) => r.pos)).toEqual([0]);
  expect(folded).toBe(2);
});

test("a run of only repeats folds to nothing visible", () => {
  const { folded, visible } = fold({ items: [item(0, true), item(1, true)] });
  expect(visible).toEqual([]);
  expect(folded).toBe(2);
});

test("show, expand-all, a filter or an opened repeat reveal every chip", () => {
  for (const over of [
    { showRepeats: true },
    { allOpen: true },
    { forceExpand: true },
    { opened: (r: RunItem) => r.pos === 2 },
  ]) {
    const { folded, visible } = fold(over);
    expect(visible.map((r) => r.pos)).toEqual([0, 1, 2]);
    expect(folded).toBe(0);
  }
});

test("an opened fresh chip does not reveal repeats", () => {
  expect(fold({ opened: (r) => r.pos === 0 }).folded).toBe(2);
});

const runOf = (flags: string) => [...flags].map((c, i) => item(i, c === "R"));

const windowOf = (flags: string, showAll: boolean) => {
  const { folded, hidden, shown } = chipWindow({
    allOpen: false,
    forceExpand: false,
    items: runOf(flags),
    opened: () => false,
    showAll,
  });
  return { folded, hidden, shown: shown.map((r) => r.pos) };
};

test("one click on 'unchanged' shows every chip, hiding none", () => {
  expect(windowOf("FRFFFFF", false)).toEqual({
    folded: 1,
    hidden: 0,
    shown: [0, 2, 3, 4, 5, 6],
  });
  expect(windowOf("FRFFFFF", true)).toEqual({
    folded: 0,
    hidden: 0,
    shown: [0, 1, 2, 3, 4, 5, 6],
  });
});

test("a mostly folded run needs one click, not two", () => {
  expect(windowOf("FFFRRRRRR", true)).toEqual({
    folded: 0,
    hidden: 0,
    shown: [0, 1, 2, 3, 4, 5, 6, 7, 8],
  });
});
