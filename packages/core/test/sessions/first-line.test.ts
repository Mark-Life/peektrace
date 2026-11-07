import { describe, expect, test } from "bun:test";
import { firstLine } from "../../src/services/sessions/tokens";

/** The split-based version, kept as an oracle. */
const oldFirstLine = (s: string, lineCap = 200): string => {
  const line = (s || "").split("\n").find((l) => l.trim().length > 0) ?? "";
  const trimmed = line.trim();
  return trimmed.length > lineCap ? `${trimmed.slice(0, lineCap)}…` : trimmed;
};

const SAMPLES = [
  "",
  "\n",
  "\n\n",
  "   \n\t\n",
  "\n\n  hello world  \nsecond",
  "  \n \t \nfirst\nsecond",
  "line one\r\nline two",
  "\r\n\r\nafter crlf\r\n",
  "no newline at all",
  "only at the end\n",
  "\n\n\nthe line is last",
  "x".repeat(300),
  `\n  ${"y".repeat(300)}  \nz`,
  "a\n",
];

describe("firstLine", () => {
  test("matches the split-based version on every sample", () => {
    for (const s of SAMPLES) {
      expect(firstLine(s)).toBe(oldFirstLine(s));
    }
  });

  test("honours a custom cap", () => {
    for (const s of SAMPLES) {
      for (const cap of [0, 1, 5, 10]) {
        expect(firstLine(s, cap)).toBe(oldFirstLine(s, cap));
      }
    }
    expect(firstLine("\n abcdefgh \n", 3)).toBe("abc…");
  });

  test("tolerates a missing string", () => {
    expect(firstLine(undefined as unknown as string)).toBe("");
  });
});
