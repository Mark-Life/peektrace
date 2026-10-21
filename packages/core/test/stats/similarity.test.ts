/** The difflib port, against golden ratios Python computed from synthetic
 * strings (`python3 -I`, `SequenceMatcher(None, a[:800], b[:800]).ratio()`),
 * and the hybrid modes that compare only the failing segment. */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  FOCUS_SEP,
  hybridSimilarity,
  normSeg,
  ratio,
  segments,
} from "../../src/services/stats/detectors/similarity";

interface Golden {
  readonly a: string;
  readonly b: string;
  readonly r: number;
}

const GOLDEN = JSON.parse(
  readFileSync(
    new URL("../fixtures/stats/difflib-golden.json", import.meta.url),
    "utf8"
  )
) as readonly Golden[];

describe("ratio", () => {
  test("agrees with difflib on every golden pair, autojunk included", () => {
    expect(GOLDEN.length).toBeGreaterThanOrEqual(28);
    expect(GOLDEN.some((g) => g.b.length >= 200)).toBe(true);
    for (const g of GOLDEN) {
      expect(Math.abs(ratio(g.a, g.b) - g.r)).toBeLessThan(1e-9);
    }
  });

  test("autojunk changes the score once b reaches 200 chars", () => {
    const a = "ab".repeat(100);
    expect(ratio(a.slice(0, 199), a.slice(0, 199))).toBe(1);
    // Every char is popular: nothing can seed a match, so nothing matches.
    expect(ratio(`x${a}`, a)).toBeLessThan(0.1);
  });
});

describe("segments", () => {
  test("splits a chain, drops noise segments, strips quotes", () => {
    expect(
      segments("cd /p && grep -rn 'a\\|b' --include='*.ts' . | head -5; echo x")
    ).toEqual(["grep -rn a\\", "b --include=*.ts ."]);
    expect(segments("echo hi")).toEqual(["echo hi"]);
    expect(normSeg(" grep  'x'   y ")).toBe("grep x y");
  });
});

describe("hybridSimilarity", () => {
  test("glob mode compares only the failing segment", () => {
    const failed = {
      text: "grep -rn foo --include=*.ts . | head; git log --oneline -30",
      focus: "grep -rn foo --include=*.ts .",
      mode: "glob" as const,
      shell: true,
    };
    const retry = "grep -rn foo --include='*.ts' .";
    expect(hybridSimilarity(failed, retry).score).toBe(1);
    expect(ratio(failed.text, retry)).toBeLessThan(0.7);
  });

  test("cnf mode also tries the retry with its binary dropped", () => {
    const failed = {
      text: "jq .name package.json",
      focus: ".name package.json",
      mode: "cnf" as const,
      shell: true,
    };
    const hit = hybridSimilarity(failed, "bun .name package.json");
    expect(hit.score).toBe(1);
    expect(hit.segment).toBe("bun .name package.json");
  });

  test("sleep mode accepts a retry that runs the blocked command", () => {
    const failed = {
      text: "sleep 90; tail -5 /tmp/out.log",
      focus: "tail -5 /tmp/out.log",
      mode: "sleep" as const,
      shell: true,
    };
    expect(
      hybridSimilarity(
        failed,
        "until grep -q DONE /tmp/out.log; do sleep 2; done"
      ).score
    ).toBe(0.9);
  });

  test("whole mode and non-shell tools compare the whole text", () => {
    const failed = {
      text: "/repo/src/a.ts",
      focus: null,
      mode: "whole" as const,
      shell: false,
    };
    expect(hybridSimilarity(failed, "/repo/src/a.ts").score).toBe(1);
    const multi = {
      text: "x",
      focus: `ls a*${FOCUS_SEP}cat b*`,
      mode: "glob" as const,
      shell: true,
    };
    expect(hybridSimilarity(multi, "cat b*").score).toBe(1);
  });
});
