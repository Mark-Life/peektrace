/** Keeping a focused block mounted when it scrolls off screen. */

import { describe, expect, test } from "bun:test";
import { keepIndex, splitPinned } from "../src/lib/virtual-range";

const items = (indexes: readonly number[]) =>
  indexes.map((index) => ({ index }));

describe("keepIndex", () => {
  test("adds a focused index outside the range, in order", () => {
    expect(keepIndex({ count: 50, indexes: [10, 11, 12], keep: 3 })).toEqual([
      3, 10, 11, 12,
    ]);
    expect(keepIndex({ count: 50, indexes: [10, 11, 12], keep: 40 })).toEqual([
      10, 11, 12, 40,
    ]);
  });

  test("leaves the range alone when nothing or a drawn item has focus", () => {
    expect(keepIndex({ count: 50, indexes: [1, 2], keep: null })).toEqual([
      1, 2,
    ]);
    expect(keepIndex({ count: 50, indexes: [1, 2], keep: 2 })).toEqual([1, 2]);
  });

  test("ignores a focused index the list no longer has", () => {
    expect(keepIndex({ count: 5, indexes: [1, 2], keep: 9 })).toEqual([1, 2]);
    expect(keepIndex({ count: 5, indexes: [1, 2], keep: -1 })).toEqual([1, 2]);
  });
});

describe("splitPinned", () => {
  test("a contiguous run has nothing pinned", () => {
    const { flow, pinned } = splitPinned(items([4, 5, 6]));
    expect(flow.map((i) => i.index)).toEqual([4, 5, 6]);
    expect(pinned).toEqual([]);
  });

  test("a stray item before or after the run is pinned", () => {
    const before = splitPinned(items([0, 7, 8, 9]));
    expect(before.pinned.map((i) => i.index)).toEqual([0]);
    expect(before.flow.map((i) => i.index)).toEqual([7, 8, 9]);
    const after = splitPinned(items([7, 8, 9, 30]));
    expect(after.pinned.map((i) => i.index)).toEqual([30]);
    expect(after.flow.map((i) => i.index)).toEqual([7, 8, 9]);
  });

  test("a lone item or an empty list stays in flow", () => {
    expect(splitPinned(items([3])).pinned).toEqual([]);
    expect(splitPinned([]).flow).toEqual([]);
  });
});
