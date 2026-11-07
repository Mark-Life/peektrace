/** Session headers for `list`, cached in memory per transcript file.
 *
 * A file's header is reused while its size and mtime match the stamp taken
 * when it was parsed, so a refresh re-reads only new or changed files.
 * OpenCode sessions live in SQLite rather than one file each and are always
 * re-read.
 */
import type { FileSystem } from "@effect/platform";
import { Effect, Option, Ref } from "effect";
import type { AgentId } from "../agent-id";
import type {
  AgentRegistryShape,
  SessionFileRef,
  TranscriptPayload,
} from "../agents";
import { PARSERS } from "./parsers";
import type { SessionParser } from "./parsers/types";
import type { SessionHeader } from "./schema";

const LIST_CONCURRENCY = 8;

/** One cached header and the file stamp it was built from. */
interface HeaderEntry {
  readonly header: SessionHeader | null;
  readonly mtimeMs: number;
  readonly sizeBytes: number;
}

/** Cached headers per agent, keyed by `entryKey`. */
export type HeaderCache = Ref.Ref<
  ReadonlyMap<AgentId, ReadonlyMap<string, HeaderEntry>>
>;

export const makeHeaderCache = () =>
  Ref.make<ReadonlyMap<AgentId, ReadonlyMap<string, HeaderEntry>>>(new Map());

// `buildHeader` reads id and slug as well as the text.
const entryKey = (ref: SessionFileRef) => `${ref.path}\0${ref.id}\0${ref.slug}`;

/** Header from a loaded transcript, before the source stamp; null if empty. */
const toHeader = (args: {
  readonly parser: SessionParser;
  readonly ref: SessionFileRef;
  readonly payload: TranscriptPayload;
}) => {
  const { parser, ref, payload } = args;
  const { text, sizeBytes, mtimeMs } = payload;
  if (text === "" && sizeBytes === 0) {
    return null;
  }
  return parser.buildHeader({
    text,
    id: ref.id,
    slug: ref.slug,
    path: ref.path,
    sizeBytes,
    mtimeMs,
  });
};

const withSource = (
  header: SessionHeader | null,
  ref: SessionFileRef
): SessionHeader | null =>
  header && ref.source
    ? { ...header, source: ref.source.id, sourceLabel: ref.source.label }
    : header;

/** Build one header from a transcript ref, without the cache. */
const headerFor = (args: {
  readonly agents: AgentRegistryShape;
  readonly agent: AgentId;
  readonly parser: SessionParser;
  readonly ref: SessionFileRef;
}) => {
  const { agents, agent, parser, ref } = args;
  return agents.loadTranscript({ agent, ref }).pipe(
    Effect.map((payload) =>
      withSource(toHeader({ parser, ref, payload }), ref)
    ),
    Effect.orElseSucceed(() => null)
  );
};

/** What one ref yields: its header, plus the entry to keep when cacheable. */
interface Lookup {
  readonly entry?: HeaderEntry;
  readonly header: SessionHeader | null;
  readonly key: string;
}

const statStamp = (fs: FileSystem.FileSystem, path: string) =>
  fs.stat(path).pipe(
    Effect.map((info) => ({
      sizeBytes: Number(info.size),
      mtimeMs: Option.match(info.mtime, {
        onNone: () => 0,
        onSome: (d) => d.getTime(),
      }),
    })),
    Effect.option
  );

/**
 * Header for a file-backed ref: reuse the cached entry when the stamp matches,
 * else load and parse. The new entry takes the payload's own stamp, which the
 * loader reads before the text, so an append during the read leaves a stale
 * stamp and the next list parses the file again.
 */
const cachedHeaderFor = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly agent: AgentId;
  readonly parser: SessionParser;
  readonly ref: SessionFileRef;
  readonly cached: ReadonlyMap<string, HeaderEntry> | undefined;
}): Effect.Effect<Lookup> => {
  const { fs, agents, agent, parser, ref, cached } = args;
  const key = entryKey(ref);
  return Effect.gen(function* () {
    const stamp = yield* statStamp(fs, ref.path);
    if (Option.isNone(stamp)) {
      return { key, header: null };
    }
    const hit = cached?.get(key);
    if (
      hit &&
      hit.sizeBytes === stamp.value.sizeBytes &&
      hit.mtimeMs === stamp.value.mtimeMs
    ) {
      return { key, header: withSource(hit.header, ref), entry: hit };
    }
    return yield* agents.loadTranscript({ agent, ref }).pipe(
      Effect.map((payload): Lookup => {
        const header = toHeader({ parser, ref, payload });
        const entry = {
          header,
          sizeBytes: payload.sizeBytes,
          mtimeMs: payload.mtimeMs,
        };
        return { key, header: withSource(header, ref), entry };
      }),
      Effect.orElseSucceed((): Lookup => ({ key, header: null }))
    );
  });
};

/**
 * Headers for one agent's transcripts, in `listSessionFiles` order. The
 * agent's cache is replaced by the entries seen in this run, so deleted files
 * drop out. Concurrent lists race last-writer-wins; every entry is checked by
 * its stamp on read, so a race costs extra parses, never a wrong header.
 */
export const listAgentHeaders = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cache: HeaderCache;
  readonly agent: AgentId;
}): Effect.Effect<readonly SessionHeader[]> => {
  const { fs, agents, cache, agent } = args;
  const parser = PARSERS[agent];
  if (!parser) {
    return Effect.succeed([]);
  }
  const keep = (h: SessionHeader | null): h is SessionHeader => h !== null;
  if (agents.roots(agent).layout === "opencode-sqlite") {
    return agents.listSessionFiles(agent).pipe(
      Effect.flatMap((refs) =>
        Effect.forEach(
          refs,
          (ref) => headerFor({ agents, agent, parser, ref }),
          {
            concurrency: LIST_CONCURRENCY,
          }
        )
      ),
      Effect.map((headers) => headers.filter(keep))
    );
  }
  return Effect.gen(function* () {
    const refs = yield* agents.listSessionFiles(agent);
    const cached = (yield* Ref.get(cache)).get(agent);
    const lookups = yield* Effect.forEach(
      refs,
      (ref) => cachedHeaderFor({ fs, agents, agent, parser, ref, cached }),
      { concurrency: LIST_CONCURRENCY }
    );
    const fresh = new Map<string, HeaderEntry>();
    for (const { key, entry } of lookups) {
      if (entry) {
        fresh.set(key, entry);
      }
    }
    yield* Ref.update(cache, (all) => new Map(all).set(agent, fresh));
    return lookups.map((l) => l.header).filter(keep);
  });
};
