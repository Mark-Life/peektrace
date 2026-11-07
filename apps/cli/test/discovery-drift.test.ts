/** Guards discovery surfaces against phrases from older product states. */
import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { Glob } from "bun";

const ROOT = join(import.meta.dirname, "../../..");
const STALE = [
  "not on npm",
  "only channel",
  "ONLY distribution channel",
  "npm (not published)",
  "shows the gap",
  "memories & sessions",
  "peektrace-<platform>-<arch>",
];
const GLOBS = [
  "README.md",
  "apps/cli/README.md",
  "apps/cli/src/index.ts",
  "apps/cli/scripts/*.ts",
  "apps/web/src/**/*.{ts,tsx}",
  ".github/workflows/*.yml",
];

describe("discovery surfaces", () => {
  test("contain no stale product phrases", async () => {
    const hits: string[] = [];
    for (const pattern of GLOBS) {
      for await (const rel of new Glob(pattern).scan({ cwd: ROOT })) {
        const text = await Bun.file(join(ROOT, rel)).text();
        for (const phrase of STALE) {
          if (text.includes(phrase)) {
            hits.push(`${rel}: ${phrase}`);
          }
        }
      }
    }
    expect(hits).toEqual([]);
  });
});
