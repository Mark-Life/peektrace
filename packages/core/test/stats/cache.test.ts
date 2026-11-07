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
  makeCache,
  mergeIndex,
} from "../../src/services/stats/cache";
import { EXTRACTOR_VERSION } from "../../src/services/stats/schema";

const entry = (path: string, rows = 1): CacheEntry => ({
  extractorVersion: EXTRACTOR_VERSION,
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
  test("a full pass replaces the index", () => {
    const next = mergeIndex({
      prev,
      entries: [entry("/a/1.jsonl", 5)],
      scanned: new Set(["/a/1.jsonl"]),
      full: true,
    });
    expect(byPath(next)).toEqual({ "/a/1.jsonl": 5 });
  });

  test("a scoped pass keeps paths outside its scope", () => {
    const next = mergeIndex({
      prev,
      entries: [entry("/b/1.jsonl", 7), entry("/b/2.jsonl", 2)],
      scanned: new Set(["/b/1.jsonl", "/b/2.jsonl"]),
      full: false,
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
      full: false,
    });
    expect(byPath(next)).toEqual({ "/a/1.jsonl": 1, "/a/2.jsonl": 1 });
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
