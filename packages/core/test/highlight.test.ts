/** The shared tokenizer behind inspector code blocks and TUI bodies. */
import { describe, expect, test } from "bun:test";
import {
  type CodeLine,
  isHighlightable,
  plainLines,
  tokenizeLines,
} from "../src/highlight";

const shape = (lines: CodeLine[]) =>
  lines.map((line) =>
    line.tokens.map((t) =>
      t.className ? `${t.className}:${t.value}` : t.value
    )
  );

describe("tokenizeLines", () => {
  test("classifies TypeScript tokens", () => {
    expect(
      shape(tokenizeLines("const x: number = 1 // hi", "typescript"))
    ).toEqual([
      [
        "keyword:const",
        " x: ",
        "type:number",
        " = ",
        "number:1",
        " ",
        "comment:// hi",
      ],
    ]);
  });

  test("splits multi-line tokens into one entry per line", () => {
    const lines = tokenizeLines("/* a\nb */\nlet y", "typescript");
    expect(shape(lines)).toEqual([
      ["comment:/* a"],
      ["comment:b */"],
      ["keyword:let", " y"],
    ]);
    expect(lines.map((l) => l.key)).toEqual(["line-0", "line-1", "line-2"]);
  });

  test("keeps empty lines", () => {
    expect(tokenizeLines('{\n\n  "a": 1\n}', "json")).toHaveLength(4);
  });

  test("tokenizes bash, json and markdown", () => {
    const flat = (code: string, lang: Parameters<typeof tokenizeLines>[1]) =>
      shape(tokenizeLines(code, lang)).flat();
    expect(flat('echo "hi"', "bash")).toContain('string:"hi"');
    expect(flat('{"a": 1}', "json")).toContain("number:1");
    expect(flat("# Title", "markdown")).toContain("heading:# Title");
  });

  test("leaves text unclassified", () => {
    expect(shape(tokenizeLines("const x", "text"))).toEqual([["const x"]]);
  });
});

describe("plainLines", () => {
  test("emits one classless token per non-empty line", () => {
    expect(shape(plainLines("a\n\nb"))).toEqual([["a"], [], ["b"]]);
  });
});

describe("isHighlightable", () => {
  test("caps linear tokenizers at 1 MB", () => {
    expect(isHighlightable("x".repeat(1_000_000), "json")).toBe(true);
    expect(isHighlightable("x".repeat(1_000_001), "typescript")).toBe(false);
  });

  test("caps bash at 20 KB", () => {
    expect(isHighlightable("x".repeat(20_000), "bash")).toBe(true);
    expect(isHighlightable("x".repeat(20_001), "bash")).toBe(false);
  });

  test("bash at the cap stays fast on heredoc-heavy input", () => {
    const code = "cat <<'EOF'\na << b\nEOF\n".repeat(1000).slice(0, 20_000);
    expect(isHighlightable(code, "bash")).toBe(true);
    const start = performance.now();
    tokenizeLines(code, "bash");
    expect(performance.now() - start).toBeLessThan(500);
  });
});
