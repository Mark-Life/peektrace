import { expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { TranscriptRowRef } from "../src/lib/chat-lane";
import { segmentsOf } from "../src/routes/sessions/chat-context-run";

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
