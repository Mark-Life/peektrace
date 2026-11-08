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
 * A pass over every agent replaces the index and deletes shards it no longer
 * names, so deleted transcripts stop costing disk. A narrower pass (one agent,
 * one session) merges: it replaces only the paths it walked and keeps the rest.
 *
 * Beside it, `~/.peektrace/rule-lines.json` holds when each memory line was
 * first seen (`rule-dates.ts`). It is history, not derived data, so it sits
 * outside the cache dir and `clear` keeps it.
 *
 * Each entry records `lastFactMs`, the newest fact time in its shard. A fresh
 * shard whose every fact predates the window is not read at all; it still
 * counts as cached and keeps its index entry, so a narrow `--days` run never
 * drops or prunes it.
 *
 * Shards are always redacted, because `extract.ts` redacts at mint time. A
 * caller asking for raw text bypasses the cache instead of writing raw text
 * down. Aggregation reruns on every report over every shard that can hold an
 * in-window fact: one new file changes shares and ranks, so nothing downstream
 * of extraction is cached.
 */
import { createHash } from "node:crypto";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { FileSystem } from "@effect/platform";
import { Context, Effect, Layer } from "effect";
import type { FileStat } from "../fs";
import { StatsCacheError } from "./errors";
import { EMPTY_LEDGER, type LineLedger } from "./rule-dates";
import { EXTRACTOR_VERSION, type StatsFact } from "./schema";
import type { TranscriptRef } from "./walk";

/** Bumped when the on-disk cache layout changes, invalidating every shard. */
const INDEX_VERSION = 1;

/** Shard-name length. Collisions are caught by comparing `entry.path`. */
const SHARD_HASH_LEN = 16;

/** One cached transcript summary. */
export interface CacheEntry {
  readonly extractorVersion: number;
  /**
   * Newest fact time in the shard, in epoch ms. Null when any fact is undated,
   * because an undated fact counts as inside every window. 0 for no facts.
   */
  readonly lastFactMs: number | null;
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

/** A fact's own time in epoch ms, or null when it is undated or unparsable. */
export const factMs = (fact: StatsFact): number | null => {
  const ts = fact.row === "session" ? fact.startedAt : fact.ts;
  if (ts === null) {
    return null;
  }
  const ms = Date.parse(ts);
  return Number.isNaN(ms) ? null : ms;
};

/** Newest fact time, or null as soon as one fact is undated. */
export const lastFactMsOf = (facts: readonly StatsFact[]): number | null => {
  let last = 0;
  for (const fact of facts) {
    const ms = factMs(fact);
    if (ms === null) {
      return null;
    }
    last = Math.max(last, ms);
  }
  return last;
};

/**
 * True when no fact in the shard can fall inside a window starting at
 * `fromMs`, so reading it would fold nothing.
 */
export const isPastWindow = (entry: CacheEntry, fromMs: number): boolean =>
  entry.lastFactMs !== null && entry.lastFactMs < fromMs;

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

/**
 * The index to commit after a scoped pass. Drops each path the pass walked (a
 * file that was skipped or not cacheable keeps no stale entry), then adds what
 * it wrote, leaving every path outside its scope untouched.
 */
export const mergeIndex = (args: {
  readonly prev: ReadonlyMap<string, CacheEntry>;
  readonly entries: readonly CacheEntry[];
  readonly scanned: ReadonlySet<string>;
}): readonly CacheEntry[] => {
  const { prev, entries, scanned } = args;
  const next = new Map(prev);
  for (const path of scanned) {
    next.delete(path);
  }
  for (const entry of entries) {
    next.set(entry.path, entry);
  }
  return [...next.values()];
};

/** Per-file fact cache, keyed on path + mtime + size + extractor version. */
export interface StatsCacheShape {
  /** Drop the whole cache dir. */
  readonly clear: () => Effect.Effect<void, StatsCacheError>;
  /** Replace the index with `entries`. Written via a temp file + rename. */
  readonly commit: (
    entries: readonly CacheEntry[]
  ) => Effect.Effect<void, StatsCacheError>;
  /** The memory-line ledger, written via a temp file + rename. */
  readonly commitLines: (
    ledger: LineLedger
  ) => Effect.Effect<void, StatsCacheError>;
  /** The index, or an empty map when absent, unreadable or version-stale. */
  readonly load: () => Effect.Effect<ReadonlyMap<string, CacheEntry>>;
  /** The memory-line ledger, or an empty one when absent or unreadable. */
  readonly loadLines: () => Effect.Effect<LineLedger>;
  /** Delete every shard not named in `keep`. Never fails: a miss is retried. */
  readonly prune: (keep: ReadonlySet<string>) => Effect.Effect<void>;
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

/** Bumped when the ledger's layout changes, which starts it over. */
const LEDGER_VERSION = 1;

interface LedgerFile extends LineLedger {
  readonly version: number;
}

const parseLedger = (raw: string): LineLedger => {
  const parsed = JSON.parse(raw) as Partial<LedgerFile>;
  return parsed.version === LEDGER_VERSION && parsed.files
    ? { files: parsed.files }
    : EMPTY_LEDGER;
};

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

/** Build the cache over `root`. The live layer picks `root` from the env. */
export const makeCache = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly root: string;
}): StatsCacheShape => {
  const { fs, root } = args;
  const indexPath = join(root, "index.json");
  const shardsDir = join(root, "shards");
  const linesPath = join(dirname(root), "rule-lines.json");
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
        lastFactMs: lastFactMsOf(facts),
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

  const loadLines: StatsCacheShape["loadLines"] = () =>
    fs.readFileString(linesPath).pipe(
      Effect.map(parseLedger),
      Effect.orElseSucceed(() => EMPTY_LEDGER)
    );

  const commitLines: StatsCacheShape["commitLines"] = (ledger) => {
    const file: LedgerFile = { version: LEDGER_VERSION, files: ledger.files };
    const tmp = `${linesPath}.tmp`;
    return fs.makeDirectory(dirname(linesPath), { recursive: true }).pipe(
      Effect.andThen(() => fs.writeFileString(tmp, JSON.stringify(file))),
      Effect.andThen(() => fs.rename(tmp, linesPath)),
      Effect.mapError(failed(linesPath))
    );
  };

  const clear: StatsCacheShape["clear"] = () =>
    fs.remove(root, { recursive: true }).pipe(
      Effect.orElseSucceed(() => undefined),
      Effect.asVoid
    );

  const prune: StatsCacheShape["prune"] = (keep) =>
    fs.readDirectory(shardsDir).pipe(
      Effect.flatMap((names) =>
        Effect.forEach(
          names.filter((n) => n.endsWith(".jsonl") && !keep.has(n)),
          (n) =>
            fs
              .remove(join(shardsDir, n))
              .pipe(Effect.orElseSucceed(() => undefined)),
          { discard: true }
        )
      ),
      Effect.orElseSucceed(() => undefined)
    );

  return {
    root,
    load,
    loadLines,
    read,
    write,
    commit,
    commitLines,
    clear,
    prune,
  };
};

/** Live layer: the cache dir resolves at layer construction, never at import. */
export const StatsCacheLive = Layer.effect(
  StatsCache,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    return makeCache({ fs, root: statsCacheRoot(process.env, homedir()) });
  })
);
