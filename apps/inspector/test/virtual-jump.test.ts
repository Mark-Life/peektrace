/** When a link lands on its block, and when it tries again. */

import { describe, expect, test } from "bun:test";
import { doneAfter, jumpDue } from "../src/lib/virtual-jump";

describe("jumpDue", () => {
  test("a named, shown target that has not landed is due", () => {
    expect(jumpDue({ done: null, index: 4, target: "a" })).toBe(true);
    expect(jumpDue({ done: "b", index: 4, target: "a" })).toBe(true);
  });

  test("no target, a landed target or a hidden one is not due", () => {
    expect(jumpDue({ done: null, index: 4, target: null })).toBe(false);
    expect(jumpDue({ done: "a", index: 4, target: "a" })).toBe(false);
    expect(jumpDue({ done: null, index: -1, target: "a" })).toBe(false);
  });
});

describe("doneAfter", () => {
  test("a landed jump stays done, so a layout switch does not land again", () => {
    const done = doneAfter({ landed: true, target: "a" });
    expect(jumpDue({ done, index: 4, target: "a" })).toBe(false);
  });

  test("a jump cut short before landing runs again", () => {
    const done = doneAfter({ landed: false, target: "a" });
    expect(done).toBeNull();
    expect(jumpDue({ done, index: 4, target: "a" })).toBe(true);
  });
});
