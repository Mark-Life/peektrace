/** One Tab stop for the session list, and how keys move it. */

import { describe, expect, test } from "bun:test";
import { rovingStop, stepIndex } from "../src/lib/roving";

describe("rovingStop", () => {
  const ids = ["a", "b", "c"];

  test("the focused row wins while it is listed", () => {
    expect(rovingStop({ ids, focusedId: "c", selectedId: "b" })).toBe("c");
  });

  test("then the open session, then the first row", () => {
    expect(rovingStop({ ids, focusedId: "gone", selectedId: "b" })).toBe("b");
    expect(rovingStop({ ids, focusedId: null, selectedId: "gone" })).toBe("a");
    expect(rovingStop({ ids, focusedId: null, selectedId: null })).toBe("a");
  });

  test("an empty list has no stop", () => {
    expect(rovingStop({ ids: [], focusedId: "a", selectedId: "a" })).toBeNull();
  });
});

describe("stepIndex", () => {
  test("down and up, by arrow or j / k", () => {
    expect(stepIndex({ count: 5, index: 1, key: "ArrowDown" })).toBe(2);
    expect(stepIndex({ count: 5, index: 1, key: "j" })).toBe(2);
    expect(stepIndex({ count: 5, index: 1, key: "ArrowUp" })).toBe(0);
    expect(stepIndex({ count: 5, index: 1, key: "k" })).toBe(0);
  });

  test("clamps at both ends instead of wrapping", () => {
    expect(stepIndex({ count: 5, index: 4, key: "j" })).toBe(4);
    expect(stepIndex({ count: 5, index: 0, key: "k" })).toBe(0);
  });

  test("Home and End jump to the ends", () => {
    expect(stepIndex({ count: 5, index: 2, key: "Home" })).toBe(0);
    expect(stepIndex({ count: 5, index: 2, key: "End" })).toBe(4);
  });

  test("other keys and empty lists do nothing", () => {
    expect(stepIndex({ count: 5, index: 2, key: "Enter" })).toBeNull();
    expect(stepIndex({ count: 0, index: 0, key: "j" })).toBeNull();
  });
});
