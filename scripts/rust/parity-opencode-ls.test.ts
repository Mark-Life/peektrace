import { describe, expect, test } from "bun:test";
import { changedPaths, lsRowDiffs } from "./parity-opencode-ls";

const row = (id: string, updatedAt = "t0") => ({
  id,
  path: `/d#${id}`,
  updatedAt,
});

describe("lsRowDiffs", () => {
  test("reports rows one side drops or invents", () => {
    expect(
      lsRowDiffs([row("a"), row("b")], [row("a"), row("c")], new Set())
    ).toEqual(["only in ts: /d#b", "only in rust: /d#c"]);
  });

  test("skips changed sessions only", () => {
    const changed = new Set(["/d#b", "/d#c"]);
    expect(
      lsRowDiffs([row("a"), row("b")], [row("a"), row("c")], changed)
    ).toEqual([]);
    expect(lsRowDiffs([row("a"), row("b")], [row("c")], changed)).toEqual([
      "only in ts: /d#a",
    ]);
  });

  test("reports a differing row and the order", () => {
    expect(
      lsRowDiffs([row("a"), row("b")], [row("b", "t1"), row("a")], new Set())
    ).toEqual([
      '/d#b line 1: ts="{\\"id\\":\\"b\\",\\"path\\":\\"/d#b\\",\\"updatedAt\\":\\"t0\\"}" rust="{\\"id\\":\\"b\\",\\"path\\":\\"/d#b\\",\\"updatedAt\\":\\"t1\\"}"',
      "order differs at 0: ts=/d#a rust=/d#b",
    ]);
  });
});

describe("changedPaths", () => {
  test("a session absent before counts as changed, an unchanged one does not", () => {
    const before = new Map([
      ["a", "s"],
      ["b", "s"],
    ]);
    const now: Record<string, string> = { a: "s", b: "s2", c: "s" };
    const stamp = (id: string) => now[id] ?? "";
    expect(changedPaths([row("a"), row("b"), row("c")], before, stamp)).toEqual(
      new Set(["/d#b", "/d#c"])
    );
  });
});
