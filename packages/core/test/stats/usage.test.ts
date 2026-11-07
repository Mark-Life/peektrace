/** Per-turn usage facts: the base the usage-over-time view folds. */
import { afterEach, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect } from "effect";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { makeCache } from "../../src/services/stats/cache";
import { extractFacts } from "../../src/services/stats/extract";
import { makeRedactor } from "../../src/services/stats/redact";
import { isCallFact } from "../../src/services/stats/schema";
import { refOf } from "./fixture-session";

const PATH = new URL(
  "../fixtures/sessions-claude/retries.jsonl",
  import.meta.url
).pathname;

const parsed = parseClaudeSession({
  text: readFileSync(PATH, "utf8"),
  path: PATH,
  sessionId: "retries",
});

const facts = extractFacts({
  ref: refOf("retries"),
  parsed,
  project: "proj",
  redact: makeRedactor({ enabled: true }),
});

const usage = facts.filter((f) => f.row === "usage");

let root = "";

afterEach(() => {
  if (root) {
    rmSync(root, { recursive: true, force: true });
  }
});

describe("usage facts", () => {
  test("one usage fact per parsed turn", () => {
    expect(parsed.turns.length).toBeGreaterThan(0);
    expect(usage).toHaveLength(parsed.turns.length);
    expect(usage.map((f) => f.req)).toEqual(
      parsed.turns.map((t) => t.requestId)
    );
  });

  test("isCallFact accepts only tool and bash rows", () => {
    const first = usage[0];
    expect(first).toBeDefined();
    expect(first && isCallFact(first)).toBe(false);
    expect(facts.filter(isCallFact).every((f) => f.row !== "usage")).toBe(true);
  });

  test("usage facts survive a cache round trip", async () => {
    root = mkdtempSync(join(tmpdir(), "peektrace-cache-"));
    const back = await Effect.runPromise(
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const cache = makeCache({ fs, root });
        const entry = yield* cache.write({
          ref: refOf("retries"),
          stat: { mtimeMs: 1, size: 1 },
          facts,
        });
        return entry === null ? [] : yield* cache.read(entry);
      }).pipe(Effect.provide(BunFileSystem.layer))
    );
    expect(back.filter((f) => f.row === "usage")).toEqual(usage);
  });
});
