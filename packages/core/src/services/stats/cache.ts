/**
 * The fact cache: one shard per transcript, keyed on path + mtime + size.
 *
 * Lives under `PEEKTRACE_DIR` (default `~/.peektrace`):
 *
 *   ~/.peektrace/stats/index.json          one entry per transcript
 *   ~/.peektrace/stats/shards/<hash>.jsonl the facts extracted from it
 *
 * A shard is re-extracted when `mtimeMs` differs, `size` differs, the recorded
 * path differs (a hash collision), or `EXTRACTOR_VERSION` moved. Everything else
 * is read back, so a rerun over an unchanged corpus parses nothing.
 *
 * Shards are always redacted, because `extract.ts` redacts at mint time. A
 * caller asking for raw text bypasses the cache instead of writing raw text
 * down. Aggregation still runs over every shard on every report: one new file
 * changes shares and ranks, so nothing downstream of extraction is cached.
 */
import { createHash } from "node:crypto";
import { homedir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { Context, Effect, Layer } from "effect";
import type { FileStat } from "../fs";
import { StatsCacheError } from "./errors";
import { EXTRACTOR_VERSION, type StatsFact } from "./schema";
import type { TranscriptRef } from "./walk";

/** Bumped when the on-disk cache layout changes, invalidating every shard. */
const INDEX_VERSION = 1;

/** Shard-name length. Collisions are caught by comparing `entry.path`. */
const SHARD_HASH_LEN = 16;

/** One cached transcript summary. */
export interface CacheEntry {
  readonly extractorVersion: number;
  readonly mtimeMs: number;
  readonly path: string;
  readonly rows: number;
  /** Shard filename under `shards/`. */
  readonly shard: string;
  readonly size: number;
}

/** The persisted index. Entries are keyed by absolute transcript path. */
interface CacheIndexFile {
  readonly entries: Record<string, CacheEntry>;
  readonly version: number;
}

/** Shard name for a transcript path. Stable across runs and platforms. */
const hashPath = (s: string): string =>
  createHash("sha1").update(s).digest("hex").slice(0, SHARD_HASH_LEN);

/**
 * True when a transcript can be cached at all. An OpenCode ref is a
 * `<dataDir>#<id>` handle with no file behind it, so its stat is 0/0 and it is
 * re-extracted every run rather than being read back as "unchanged".
 */
export const isCacheable = (stat: FileStat): boolean =>
  stat.size > 0 || stat.mtimeMs > 0;

/** True when a cached entry still describes the file on disk. */
export const isFresh = (args: {
  readonly entry: CacheEntry;
  readonly ref: TranscriptRef;
  readonly stat: FileStat;
}): boolean => {
  const { entry, ref, stat } = args;
  return (
    isCacheable(stat) &&
    entry.path === ref.path &&
    entry.mtimeMs === stat.mtimeMs &&
    entry.size === stat.size &&
    entry.extractorVersion === EXTRACTOR_VERSION
  );
};

/** Per-file fact cache, keyed on path + mtime + size + extractor version. */
export interface StatsCacheShape {
  /** Drop the whole cache dir. */
  readonly clear: () => Effect.Effect<void, StatsCacheError>;
  /** Replace the index with `entries`. Written via a temp file + rename. */
  readonly commit: (
    entries: readonly CacheEntry[]
  ) => Effect.Effect<void, StatsCacheError>;
  /** The index, or an empty map when absent, unreadable or version-stale. */
  readonly load: () => Effect.Effect<ReadonlyMap<string, CacheEntry>>;
  /** Facts of one cached transcript. Fails only on a corrupt shard. */
  readonly read: (
    entry: CacheEntry
  ) => Effect.Effect<readonly StatsFact[], StatsCacheError>;
  /** Absolute cache dir, so a CLI can name it when it reports a refresh. */
  readonly root: string;
  /** Write one transcript's facts. Returns null when the ref is not cacheable. */
  readonly write: (args: {
    readonly ref: TranscriptRef;
    readonly stat: FileStat;
    readonly facts: readonly StatsFact[];
  }) => Effect.Effect<CacheEntry | null, StatsCacheError>;
}

/** Per-file fact cache for the stats service. */
export class StatsCache extends Context.Tag("@peektrace/StatsCache")<
  StatsCache,
  StatsCacheShape
>() {}

/** The stats cache dir, under `PEEKTRACE_DIR` or `~/.peektrace`. */
export const statsCacheRoot = (env: NodeJS.ProcessEnv, home: string): string =>
  join(env.PEEKTRACE_DIR ?? join(home, ".peektrace"), "stats");

const parseIndex = (raw: string): ReadonlyMap<string, CacheEntry> => {
  const parsed = JSON.parse(raw) as Partial<CacheIndexFile>;
  if (parsed.version !== INDEX_VERSION || !parsed.entries) {
    return new Map();
  }
  return new Map(Object.entries(parsed.entries));
};

/**
 * Decode a shard. Lines are written by `write` in this process, so the format
 * is trusted and the row discriminator is the only check: schema-decoding
 * ~70,000 rows per report costs more than re-parsing a transcript.
 */
const parseShard = (raw: string): readonly StatsFact[] => {
  const out: StatsFact[] = [];
  for (const line of raw.split("\n")) {
    if (line.length === 0) {
      continue;
    }
    const fact = JSON.parse(line) as StatsFact;
    if (fact.row === "session" || fact.row === "tool" || fact.row === "bash") {
      out.push(fact);
    }
  }
  return out;
};

const makeCache = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly root: string;
}): StatsCacheShape => {
  const { fs, root } = args;
  const indexPath = join(root, "index.json");
  const shardsDir = join(root, "shards");
  const failed = (path: string) => (e: unknown) =>
    new StatsCacheError({ path, reason: String(e) });

  const load: StatsCacheShape["load"] = () =>
    fs.readFileString(indexPath).pipe(
      Effect.map(parseIndex),
      Effect.orElseSucceed(() => new Map<string, CacheEntry>())
    );

  const read: StatsCacheShape["read"] = (entry) =>
    fs
      .readFileString(join(shardsDir, entry.shard))
      .pipe(
        Effect.map(parseShard),
        Effect.mapError(failed(join(shardsDir, entry.shard)))
      );

  const write: StatsCacheShape["write"] = ({ ref, stat, facts }) => {
    if (!isCacheable(stat)) {
      return Effect.succeed(null);
    }
    const shard = `${hashPath(ref.path)}.jsonl`;
    const path = join(shardsDir, shard);
    const body = facts.map((f) => JSON.stringify(f)).join("\n");
    return fs.makeDirectory(shardsDir, { recursive: true }).pipe(
      Effect.andThen(() => fs.writeFileString(path, body)),
      Effect.as({
        path: ref.path,
        mtimeMs: stat.mtimeMs,
        size: stat.size,
        shard,
        rows: facts.length,
        extractorVersion: EXTRACTOR_VERSION,
      } satisfies CacheEntry),
      Effect.mapError(failed(path))
    );
  };

  const commit: StatsCacheShape["commit"] = (entries) => {
    const file: CacheIndexFile = {
      version: INDEX_VERSION,
      entries: Object.fromEntries(entries.map((e) => [e.path, e])),
    };
    const tmp = `${indexPath}.tmp`;
    return fs.makeDirectory(root, { recursive: true }).pipe(
      Effect.andThen(() => fs.writeFileString(tmp, JSON.stringify(file))),
      Effect.andThen(() => fs.rename(tmp, indexPath)),
      Effect.mapError(failed(indexPath))
    );
  };

  const clear: StatsCacheShape["clear"] = () =>
    fs.remove(root, { recursive: true }).pipe(
      Effect.orElseSucceed(() => undefined),
      Effect.asVoid
    );

  return { root, load, read, write, commit, clear };
};

/** Live layer: the cache dir resolves at layer construction, never at import. */
export const StatsCacheLive = Layer.effect(
  StatsCache,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    return makeCache({ fs, root: statsCacheRoot(process.env, homedir()) });
  })
);
