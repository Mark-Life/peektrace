/** Mapping a pointer over the growth timeline to the nearest snapshot. */

import { expect, test } from "bun:test";
import { clickAction, snapIndexAtX } from "../src/lib/timeline-hit";

const plot = { left: 100, plotW: 800 } as const;

test("snapshots spread evenly from edge to edge", () => {
  expect(snapIndexAtX({ ...plot, n: 5, px: 100 })).toBe(0);
  expect(snapIndexAtX({ ...plot, n: 5, px: 500 })).toBe(2);
  expect(snapIndexAtX({ ...plot, n: 5, px: 900 })).toBe(4);
});

test("the nearest snapshot wins", () => {
  // Snapshots at 100, 300, 500, …: 199 is nearer 100, 201 nearer 300.
  expect(snapIndexAtX({ ...plot, n: 5, px: 199 })).toBe(0);
  expect(snapIndexAtX({ ...plot, n: 5, px: 201 })).toBe(1);
});

test("a pointer past either edge clamps to the end snapshot", () => {
  expect(snapIndexAtX({ ...plot, n: 5, px: 0 })).toBe(0);
  expect(snapIndexAtX({ ...plot, n: 5, px: 2000 })).toBe(4);
});

test("one snapshot is always it; none is -1", () => {
  expect(snapIndexAtX({ ...plot, n: 1, px: 0 })).toBe(0);
  expect(snapIndexAtX({ ...plot, n: 1, px: 900 })).toBe(0);
  expect(snapIndexAtX({ ...plot, n: 0, px: 500 })).toBe(-1);
});

test("a mouse click jumps; a tap shows first, then jumps on the same snapshot", () => {
  expect(clickAction({ armed: -1, i: 2, touch: false })).toBe("jump");
  expect(clickAction({ armed: -1, i: 2, touch: true })).toBe("show");
  expect(clickAction({ armed: 1, i: 2, touch: true })).toBe("show");
  expect(clickAction({ armed: 2, i: 2, touch: true })).toBe("jump");
});
