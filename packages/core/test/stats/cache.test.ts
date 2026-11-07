/** Index merging and shard pruning, over a real temp dir. */
import { afterEach, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect } from "effect";
import {
  type CacheEntry,
  factMs,
  isPastWindow,
  lastFactMsOf,
  makeCache,
  mergeIndex,
} from "../../src/services/stats/cache";
import { EXTRACTOR_VERSION } from "../../src/services/stats/schema";
import { bashFact, sessionFact } from "./facts";

const entry = (path: string, rows = 1): CacheEntry => ({
  extractorVersion: EXTRACTOR_VERSION,
  lastFactMs: null,
  mtimeMs: 1,
  path,
  rows,
  shard: `${path.replaceAll("/", "_")}.jsonl`,
  size: 1,
});

const byPath = (entries: readonly CacheEntry[]) =>
  Object.fromEntries(entries.map((e) => [e.path, e.rows]));

const prev = new Map([
  ["/a/1.jsonl", entry("/a/1.jsonl")],
  ["/a/2.jsonl", entry("/a/2.jsonl")],
  ["/b/1.jsonl", entry("/b/1.jsonl")],
]);

describe("mergeIndex", () => {
  test("a scoped pass keeps paths outside its scope", () => {
    const next = mergeIndex({
      prev,
      entries: [entry("/b/1.jsonl", 7), entry("/b/2.jsonl", 2)],
      scanned: new Set(["/b/1.jsonl", "/b/2.jsonl"]),
    });
    expect(byPath(next)).toEqual({
      "/a/1.jsonl": 1,
      "/a/2.jsonl": 1,
      "/b/1.jsonl": 7,
      "/b/2.jsonl": 2,
    });
  });

  test("a scanned path with no new entry is dropped", () => {
    const next = mergeIndex({
      prev,
      entries: [],
      scanned: new Set(["/b/1.jsonl"]),
    });
    expect(byPath(next)).toEqual({ "/a/1.jsonl": 1, "/a/2.jsonl": 1 });
  });
});

describe("window skip predicate", () => {
  const T1 = "2026-08-01T00:00:00.000Z";
  const T2 = "2026-08-03T00:00:00.000Z";
  const at = (iso: string) => Date.parse(iso);
  const withLast = (lastFactMs: number | null) => ({
    ...entry("/a/1.jsonl"),
    lastFactMs,
  });

  test("factMs reads ts, or startedAt for a session row", () => {
    expect(factMs(bashFact({ ts: T1 }))).toBe(at(T1));
    expect(factMs(sessionFact({ startedAt: T2 }))).toBe(at(T2));
  });

  test("an undated or unparsable fact has no time", () => {
    expect(factMs(bashFact({ ts: null }))).toBeNull();
    expect(factMs(bashFact({ ts: "not a date" }))).toBeNull();
    expect(factMs(sessionFact({ startedAt: null }))).toBeNull();
  });

  test("lastFactMsOf is the newest time, null once a fact is undated", () => {
    const dated = [sessionFact({ startedAt: T1 }), bashFact({ ts: T2 })];
    expect(lastFactMsOf(dated)).toBe(at(T2));
    expect(lastFactMsOf([...dated, bashFact({ ts: "bad" })])).toBeNull();
    expect(lastFactMsOf([])).toBe(0);
  });

  test("only a shard wholly before the window start is skipped", () => {
    expect(isPastWindow(withLast(at(T1)), at(T2))).toBe(true);
    expect(isPastWindow(withLast(at(T2)), at(T2))).toBe(false);
    expect(isPastWindow(withLast(at(T2)), at(T1))).toBe(false);
    expect(isPastWindow(withLast(null), at(T2))).toBe(false);
    expect(isPastWindow(withLast(0), at(T1))).toBe(true);
  });
});

let root = "";

afterEach(() => {
  if (root) {
    rmSync(root, { recursive: true, force: true });
  }
});

const runPrune = (keep: ReadonlySet<string>) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      yield* makeCache({ fs, root }).prune(keep);
    }).pipe(Effect.provide(BunFileSystem.layer))
  );

describe("prune", () => {
  test("deletes unreferenced shards and leaves other files", async () => {
    root = mkdtempSync(join(tmpdir(), "peektrace-cache-"));
    const shards = join(root, "shards");
    mkdirSync(shards, { recursive: true });
    for (const name of ["keep.jsonl", "dead.jsonl", "notes.txt"]) {
      writeFileSync(join(shards, name), "");
    }
    await runPrune(new Set(["keep.jsonl"]));
    expect(readdirSync(shards).sort()).toEqual(["keep.jsonl", "notes.txt"]);
  });

  test("succeeds when no shard dir exists", async () => {
    root = mkdtempSync(join(tmpdir(), "peektrace-cache-"));
    await runPrune(new Set());
    expect(existsSync(join(root, "shards"))).toBe(false);
  });
});
