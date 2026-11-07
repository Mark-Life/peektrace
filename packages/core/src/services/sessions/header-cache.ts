/** Session headers for `list`, cached in memory per transcript file.
 *
 * A file's header is reused while its size and mtime match the stamp taken
 * when it was parsed, so a refresh re-reads only new or changed files.
 * OpenCode sessions live in SQLite rather than one file each and are always
 * re-read. A failed read is never cached, and a cache hit is reused only while
 * the file is still readable, so an unreadable file is reported on every list.
 */
import type { FileSystem } from "@effect/platform";
import { Effect, Ref } from "effect";
import type { AgentId } from "../agent-id";
import type {
  AgentRegistryShape,
  SessionFileRef,
  TranscriptPayload,
} from "../agents";
import { statFileStamp } from "../file-stamp";
import { PARSERS } from "./parsers";
import type { SessionParser } from "./parsers/types";
import type { SessionHeader, UnreadableSession } from "./schema";

/** Files read in parallel per agent, and agents listed in parallel. */
export const LIST_CONCURRENCY = 8;

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

/** What one listed ref yields. An empty file is a skip, not an error. */
export type HeaderResult =
  | { readonly _tag: "Header"; readonly header: SessionHeader }
  | { readonly _tag: "Skip" }
  | { readonly _tag: "Unreadable"; readonly session: UnreadableSession };

const skip: HeaderResult = { _tag: "Skip" };

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
): HeaderResult =>
  header
    ? {
        _tag: "Header",
        header: ref.source
          ? { ...header, source: ref.source.id, sourceLabel: ref.source.label }
          : header,
      }
    : skip;

/** Debug-log a failed read and report the ref as unreadable. */
const unreadable = (args: {
  readonly agent: AgentId;
  readonly ref: SessionFileRef;
  readonly reason: unknown;
}) =>
  Effect.logDebug("session unreadable").pipe(
    Effect.annotateLogs({ path: args.ref.path, reason: String(args.reason) }),
    Effect.as<HeaderResult>({
      _tag: "Unreadable",
      session: {
        agent: args.agent,
        project: args.ref.slug,
        path: args.ref.path,
      },
    })
  );

const logMalformed = (r: HeaderResult, ref: SessionFileRef) =>
  r._tag === "Header" && r.header.malformedLines
    ? Effect.logDebug("session has malformed lines").pipe(
        Effect.annotateLogs({
          path: ref.path,
          malformedLines: r.header.malformedLines,
        })
      )
    : Effect.void;

/** Build one header from a transcript ref, without the cache. */
const headerFor = (args: {
  readonly agents: AgentRegistryShape;
  readonly agent: AgentId;
  readonly parser: SessionParser;
  readonly ref: SessionFileRef;
}): Effect.Effect<HeaderResult> => {
  const { agents, agent, parser, ref } = args;
  return agents.loadTranscript({ agent, ref }).pipe(
    Effect.map((payload) =>
      withSource(toHeader({ parser, ref, payload }), ref)
    ),
    Effect.tap((r) => logMalformed(r, ref)),
    Effect.catchTag("TranscriptReadError", (e) =>
      unreadable({ agent, ref, reason: e.reason })
    )
  );
};

/** What one ref yields, plus the entry to keep when cacheable. */
interface Lookup {
  readonly entry?: HeaderEntry;
  readonly key: string;
  readonly result: HeaderResult;
}

/**
 * Result for a file-backed ref: reuse the cached entry when the stamp matches
 * and the file still reads, else load and parse. The load reuses the stamp
 * taken here, before the read, so an append during the read leaves a stale
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
  const failed = (reason: unknown) =>
    Effect.map(
      unreadable({ agent, ref, reason }),
      (result): Lookup => ({ key, result })
    );
  return Effect.gen(function* () {
    const stamp = yield* Effect.either(statFileStamp(fs, ref.path));
    if (stamp._tag === "Left") {
      return yield* failed(stamp.left);
    }
    const hit = cached?.get(key);
    if (
      hit &&
      hit.sizeBytes === stamp.right.sizeBytes &&
      hit.mtimeMs === stamp.right.mtimeMs
    ) {
      // A chmod keeps size and mtime, so check the file still reads.
      const access = yield* Effect.either(
        fs.access(ref.path, { readable: true })
      );
      if (access._tag === "Left") {
        return yield* failed(access.left);
      }
      return { key, result: withSource(hit.header, ref), entry: hit };
    }
    return yield* agents
      .loadTranscript({ agent, ref, stamp: stamp.right })
      .pipe(
        Effect.map((payload): Lookup => {
          const header = toHeader({ parser, ref, payload });
          const entry = {
            header,
            sizeBytes: payload.sizeBytes,
            mtimeMs: payload.mtimeMs,
          };
          return { key, result: withSource(header, ref), entry };
        }),
        Effect.tap((l) => logMalformed(l.result, ref)),
        Effect.catchTag("TranscriptReadError", (e) => failed(e.reason))
      );
  });
};

/**
 * Results for one agent's transcripts, in `listSessionFiles` order. The
 * agent's cache is replaced by the entries seen in this run, so deleted files
 * drop out. Concurrent lists race last-writer-wins; every entry is checked by
 * its stamp on read, so a race costs extra parses, never a wrong header.
 */
export const listAgentHeaders = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cache: HeaderCache;
  readonly agent: AgentId;
}): Effect.Effect<readonly HeaderResult[]> => {
  const { fs, agents, cache, agent } = args;
  const parser = PARSERS[agent];
  if (!parser) {
    return Effect.succeed([]);
  }
  if (agents.roots(agent).layout === "opencode-sqlite") {
    return agents
      .listSessionFiles(agent)
      .pipe(
        Effect.flatMap((refs) =>
          Effect.forEach(
            refs,
            (ref) => headerFor({ agents, agent, parser, ref }),
            { concurrency: LIST_CONCURRENCY }
          )
        )
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
    return lookups.map((l) => l.result);
  });
};
