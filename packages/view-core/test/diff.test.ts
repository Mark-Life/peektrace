/** The line diff behind the inspector's edit rows.
 *
 * The interesting property is alignment: a one-line change must not read as
 * "everything removed, everything added", or an edit row tells the reader
 * nothing the raw arguments did not.
 */
import { describe, expect, test } from "bun:test";
import { diffLines } from "../src/diff";

const shape = (before: string, after: string) =>
  diffLines(before, after).rows.map((r) =>
    r.kind === "gap" ? `gap:${r.count}` : `${r.kind}:${r.text}`
  );

describe("aligning a before/after pair", () => {
  test("a changed line leaves its neighbours as context", () => {
    expect(shape("a\nb\nc", "a\nB\nc")).toEqual([
      "ctx:a",
      "del:b",
      "add:B",
      "ctx:c",
    ]);
  });

  test("counts what changed, not what it printed", () => {
    const { added, removed } = diffLines("a\nb\nc", "a\nB\nc");
    expect({ added, removed }).toEqual({ added: 1, removed: 1 });
  });

  test("numbers each side against its own text", () => {
    const rows = diffLines("a\nb", "a\nx\nb").rows;
    expect(rows.map((r) => [r.kind, r.oldNo, r.newNo])).toEqual([
      ["ctx", 1, 1],
      ["add", undefined, 2],
      ["ctx", 2, 3],
    ]);
  });

  test("an insertion adds without removing anything", () => {
    expect(shape("a\nb", "a\nx\nb")).toEqual(["ctx:a", "add:x", "ctx:b"]);
  });

  test("writing into emptiness is all additions", () => {
    expect(shape("", "a\nb")).toEqual(["add:a", "add:b"]);
  });

  test("identical text has nothing to show", () => {
    expect(shape("a\nb", "a\nb")).toEqual(["gap:2"]);
  });
});

describe("keeping a long diff readable", () => {
  const long = (n: number, mark = "") =>
    Array.from({ length: n }, (_, i) => `line ${i}${mark}`).join("\n");

  test("unchanged stretches collapse to one counted gap", () => {
    const before = long(20);
    const after = before.replace("line 10", "line 10 changed");
    const rows = diffLines(before, after).rows;
    expect(rows.filter((r) => r.kind === "gap")).toEqual([
      { count: 7, kind: "gap", text: "" },
      { count: 6, kind: "gap", text: "" },
    ]);
    expect(rows.filter((r) => r.kind === "ctx")).toHaveLength(6);
  });

  test("a diff too large to align still says what it replaced", () => {
    const { added, removed, rows } = diffLines(long(1200), long(1200, "!"));
    expect({ added, removed }).toEqual({ added: 1200, removed: 1200 });
    expect(rows.at(0)?.kind).toBe("del");
    expect(rows.at(-1)?.kind).toBe("add");
  });
});
