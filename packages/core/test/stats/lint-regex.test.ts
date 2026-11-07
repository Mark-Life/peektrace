import { describe, expect, test } from "bun:test";
import {
  LINT_RE,
  scanOutput,
} from "../../src/services/stats/detectors/failures";

/** The pattern before the lookbehind anchor, kept as an oracle. */
const OLD_LINT_RE =
  /(?:[\w./-]+:\d+:\d+\s+lint\/[a-z]+\/\w+|\bFound \d+ errors?\b)/;

const shell = (text: string) =>
  scanOutput({ tool: "Bash", text, isError: false });

const timed = (fn: () => void) => {
  const start = performance.now();
  fn();
  return performance.now() - start;
};

const matchOf = (re: RegExp, s: string) => {
  const m = re.exec(s);
  return m ? { index: m.index, text: m[0] } : null;
};

/** Park-Miller PRNG so the fuzz corpus is the same on every run. */
const seeded = (seed: number) => {
  let a = seed;
  return () => {
    a = (a * 16_807) % 2_147_483_647;
    return (a - 1) / 2_147_483_646;
  };
};

const TOKENS = [
  "x",
  ".",
  "/",
  "-",
  ":",
  "1",
  " ",
  "\n",
  "lint/",
  "a/",
  "b",
  "Found",
  "errors",
  // composite pieces so a fair share of strings actually match
  ":1:2 ",
  "lint/a/b",
  "Found 2 errors",
];

describe("lint regex", () => {
  test("a long run of path chars scans in linear time", () => {
    const runs = [
      `${"x".repeat(100_000)} lint/a/b`,
      "a1:".repeat(30_000),
      `${"-./".repeat(30_000)}:1:2 lint`,
    ];
    for (const text of runs) {
      expect(timed(() => shell(text))).toBeLessThan(200);
    }
  });

  test("a match at the end of a long run still fires", () => {
    expect(shell(`${"x".repeat(50_000)}:1:2 lint/style/useConst`).reason).toBe(
      "lint"
    );
  });

  // The old leftmost match never starts right after a path char: that char
  // would extend the greedy run and give a match one offset earlier. So the
  // lookbehind only drops attempts that could never be the leftmost match.
  test("hand samples match the old pattern exactly", () => {
    const samples = [
      "src/x.ts:3:1 lint/style/useConst  FIXABLE",
      "packages/a-b/c.d.ts:120:44 lint/suspicious/noExplicitAny ━━━",
      "Checked 40 files in 12ms. Found 3 errors.",
      "Found 1 error",
      "my dir/file.ts:1:2 lint/a/b",
      "a.ts:1:1 lint/x/y at offset zero",
      "prefix.src/x.ts:9:9 lint/a/b",
      "prefix/src/x.ts:9:9 lint/a/b",
      "prefix-src/x.ts:9:9 lint/a/b",
      "(src/x.ts:9:9 lint/a/b)",
      "x.ts:1:2\tlint/a/b",
      "x.ts:1:2\nlint/a/b",
      "the workflow writes to lint/ and exits",
      "nothing to see here",
      "",
    ];
    for (const s of samples) {
      expect(matchOf(LINT_RE, s)).toEqual(matchOf(OLD_LINT_RE, s));
    }
  });

  test("random token strings match the old pattern exactly", () => {
    const rand = seeded(83);
    let hits = 0;
    for (let i = 0; i < 5000; i += 1) {
      const len = Math.floor(rand() * 41);
      let s = "";
      for (let j = 0; j < len; j += 1) {
        s += TOKENS[Math.floor(rand() * TOKENS.length)];
      }
      const want = matchOf(OLD_LINT_RE, s);
      if (want) {
        hits += 1;
      }
      expect(matchOf(LINT_RE, s)).toEqual(want);
    }
    // the corpus must exercise the match path, not only misses
    expect(hits).toBeGreaterThan(0);
  });
});
