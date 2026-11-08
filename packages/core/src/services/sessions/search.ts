/** Search every session's text for a phrase.
 *
 * Candidates come from the header cache (agent, project, since), newest first.
 * Transcripts load a few at a time and the scan stops once `limit + 1`
 * sessions matched, so a common phrase in recent sessions reads little.
 */
import { homedir } from "node:os";
import type { FileSystem } from "@effect/platform";
import { Effect, Ref, Stream } from "effect";
import type { AgentRegistryShape, SessionFileRef } from "../agents";
import { type HeaderCache, LIST_CONCURRENCY } from "./header-cache";
import { listSessions } from "./listing";
import { byUpdatedDesc, timeOf } from "./order";
import type { SessionHeader } from "./schema";
import { matchTranscript, redactDisplay } from "./search-match";
import type {
  SearchRequest,
  SearchSession,
  SessionSearchResult,
} from "./search-schema";

const DEFAULT_LIMIT = 20;
const HITS_PER_SESSION = 3;

const refOf = (h: SessionHeader): SessionFileRef => ({
  id: h.id,
  path: h.path,
  slug: h.project,
  ...(h.source
    ? { source: { id: h.source, label: h.sourceLabel ?? h.source } }
    : {}),
});

interface Counters {
  readonly bytes: Ref.Ref<number>;
  readonly failed: Ref.Ref<number>;
  readonly scanned: Ref.Ref<number>;
}

/** Load one transcript and match it; a failed read counts as unreadable. */
const scanOne = (args: {
  readonly agents: AgentRegistryShape;
  readonly counters: Counters;
  readonly header: SessionHeader;
  readonly q: string;
  readonly redact: boolean;
}): Effect.Effect<SearchSession | undefined> => {
  const { agents, counters, header, q, redact } = args;
  return agents
    .loadTranscript({ agent: header.agent, ref: refOf(header) })
    .pipe(
      Effect.flatMap(({ text }) =>
        Ref.update(counters.scanned, (n) => n + 1).pipe(
          Effect.zipRight(Ref.update(counters.bytes, (n) => n + text.length)),
          Effect.as(
            matchTranscript({
              text,
              q,
              redact,
              maxHits: HITS_PER_SESSION,
              home: homedir(),
            })
          )
        )
      ),
      Effect.map(({ hits, matchCount }): SearchSession | undefined =>
        matchCount === 0
          ? undefined
          : {
              id: header.id,
              agent: header.agent,
              project: redact
                ? redactDisplay(header.project, homedir())
                : header.project,
              ...(header.title === undefined
                ? {}
                : {
                    title: redact
                      ? redactDisplay(header.title, homedir())
                      : header.title,
                  }),
              ...(header.updatedAt === undefined
                ? {}
                : { updatedAt: header.updatedAt }),
              ...(header.source === undefined ? {} : { source: header.source }),
              matchCount,
              hits,
            }
      ),
      Effect.catchAll(() =>
        Ref.update(counters.failed, (n) => n + 1).pipe(Effect.as(undefined))
      )
    );
};

/** Sessions whose text holds `req.q`, newest first. */
export const searchSessions = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cache: HeaderCache;
  readonly req: SearchRequest;
}): Effect.Effect<SessionSearchResult> => {
  const { fs, agents, cache, req } = args;
  const { q, agent, project, redact = true } = req;
  const limit =
    req.limit === undefined || req.limit < 0 ? DEFAULT_LIMIT : req.limit;
  const inProject = (name: string) =>
    project === undefined ||
    name === project ||
    (redact && redactDisplay(name, homedir()) === project);
  const sinceMs = req.since === undefined ? Number.NaN : Date.parse(req.since);
  if (q.trim() === "") {
    return Effect.succeed({
      sessions: [],
      truncated: false,
      scannedSessions: 0,
      unreadable: 0,
    } satisfies SessionSearchResult);
  }
  return Effect.gen(function* () {
    const started = Date.now();
    const listing = yield* listSessions({
      fs,
      agents,
      cache,
      ...(agent ? { filter: { agent } } : {}),
    });
    const candidates = listing.headers
      .filter(
        (h) =>
          inProject(h.project) &&
          (Number.isNaN(sinceMs) ||
            (timeOf(h) ?? Number.NEGATIVE_INFINITY) >= sinceMs)
      )
      .sort(byUpdatedDesc);
    const unreadableListed = listing.unreadable.filter(
      (u) => u.project === "" || inProject(u.project)
    ).length;
    const counters: Counters = {
      bytes: yield* Ref.make(0),
      failed: yield* Ref.make(0),
      scanned: yield* Ref.make(0),
    };
    const found = yield* Stream.fromIterable(candidates).pipe(
      Stream.mapEffect(
        (header) => scanOne({ agents, counters, header, q, redact }),
        { concurrency: LIST_CONCURRENCY }
      ),
      Stream.filter((s): s is SearchSession => s !== undefined),
      limit === 0 ? (s) => s : Stream.take(limit + 1),
      Stream.runCollect
    );
    const all = Array.from(found);
    const sessions = limit === 0 ? all : all.slice(0, limit);
    const scannedSessions = yield* Ref.get(counters.scanned);
    const failed = yield* Ref.get(counters.failed);
    yield* Effect.annotateCurrentSpan({
      scanned: scannedSessions,
      matched: sessions.length,
      bytes: yield* Ref.get(counters.bytes),
      ms: Date.now() - started,
    });
    return {
      sessions,
      truncated: all.length > sessions.length,
      scannedSessions,
      unreadable: unreadableListed + failed,
    } satisfies SessionSearchResult;
  }).pipe(Effect.withSpan("Sessions.search"));
};
