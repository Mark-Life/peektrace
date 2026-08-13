/**
 * StatsService: one streaming pass over the corpus, folded into detectors.
 *
 * `report` walks every transcript in path order, reads each one's facts from the
 * cache when its mtime and size are unchanged, folds them into the detectors and
 * drops the session before the next file. `drill` runs the same pass with one
 * detector also retaining the facts behind a single key. `refresh` runs it for
 * the shards alone.
 *
 * The clock is injected: `generatedAt` and the window bounds come from Effect's
 * `Clock`, never `Date.now()` inside the walk, so a test can pin it and diff two
 * reports byte for byte.
 */
import type { FileSystem } from "@effect/platform";
import { FileSystem as Fs } from "@effect/platform";
import { Clock, Context, Effect, Layer } from "effect";
import { AGENT_IDS, type AgentId } from "../agent-id";
import { AgentRegistry, type AgentRegistryShape } from "../agents";
import { PARSERS } from "../sessions/parsers";
import {
  type AccumulatorOptions,
  type CorpusTotals,
  type DrillTarget,
  type FactAccumulator,
  type FactSink,
  type FinishContext,
  makeCorpusTotals,
} from "./accumulate";
import {
  type CacheEntry,
  isFresh,
  StatsCache,
  type StatsCacheShape,
} from "./cache";
import { makeClusterTable } from "./detectors/clustering";
import { makeCorpusPanels } from "./detectors/corpus";
import { makeFailTable } from "./detectors/failures";
import { makeTimeTable } from "./detectors/timing";
import { makeWaitPanel } from "./detectors/waiting";
import { StatsRowNotFoundError, StatsScanError } from "./errors";
import { extractFacts } from "./extract";
import { makeMarkerCollector } from "./markers";
import {
  DEFAULT_DRILL_PAGE,
  DEFAULT_ROW_LIMIT,
  DEFAULT_SAMPLE_CAP,
  uniqSorted,
} from "./rank";
import { makeRedactor, type Redactor } from "./redact";
import {
  DETECTOR_VERSION,
  type DrillResult,
  type DrillTable,
  type RefreshSummary,
  type SessionMarkers,
  STATS_SCHEMA_VERSION,
  type StatsFact,
  type StatsReport,
  type StatsWindow,
} from "./schema";
import { foldCorpus, listCorpus, statOf, type TranscriptRef } from "./walk";

/** Days of history a report covers when the caller names none. */
const DEFAULT_DAYS = 60;

const MS_PER_DAY = 86_400_000;

/** A window wide enough to hold every fact: `refresh` filters nothing. */
const ALL_DAYS = 36_500;

/** Statements that travel with the numbers, whatever a UI chooses to render. */
const BASE_CAVEATS: readonly string[] = [
  "durSec is the gap between a call and its result: it includes harness overhead and approval waits",
  "silent-failure counts are a floor: a detector only sees the output the transcript stored",
  "backgrounded runs are charged the seconds they took, which understates the wait they started",
];

/** What `report` accepts. */
export interface ReportRequest {
  /** Agents to scan. Default: every agent with a parser. */
  readonly agents?: readonly AgentId[];
  /** History window in days. Default 60. */
  readonly days?: number;
  /** Ignore cached shards and re-extract every transcript. */
  readonly force?: boolean;
  /** Rows kept per table. */
  readonly limit?: number;
  /** Redact derived strings. Default true; false bypasses the cache. */
  readonly redact?: boolean;
}

/** What `drill` accepts. */
export interface DrillRequest extends ReportRequest {
  readonly key: string;
  /** First hit to return, counted from the whole match set. Default 0. */
  readonly offset?: number;
  /** Hits in the page. Default `DEFAULT_DRILL_PAGE`. */
  readonly pageSize?: number;
  readonly table: DrillTable;
}

/** What `refresh` accepts. */
export interface RefreshRequest {
  readonly agents?: readonly AgentId[];
  readonly force?: boolean;
}

/** What `markers` accepts. */
export interface MarkersRequest {
  readonly agents?: readonly AgentId[];
  readonly force?: boolean;
  /** The session id, i.e. `SessionFact.sid`. */
  readonly sid: string;
}

/** Cross-session friction, computed over every agent's transcripts. */
export interface StatsServiceShape {
  /** The rows behind one aggregate key. */
  readonly drill: (
    req: DrillRequest
  ) => Effect.Effect<DrillResult, StatsScanError | StatsRowNotFoundError>;
  /** Markers for one session, carrying the `detector` ids the tables use. */
  readonly markers: (
    req: MarkersRequest
  ) => Effect.Effect<SessionMarkers, StatsScanError>;
  /** Refresh the fact cache. `force` ignores mtimes. */
  readonly refresh: (
    req: RefreshRequest
  ) => Effect.Effect<RefreshSummary, StatsScanError>;
  /** The versioned report. */
  readonly report: (
    req: ReportRequest
  ) => Effect.Effect<StatsReport, StatsScanError>;
}

/** Cross-session friction stats. */
export class StatsService extends Context.Tag("@peektrace/StatsService")<
  StatsService,
  StatsServiceShape
>() {}

/** The requested agents that are supported and have a parser, in matrix order. */
const scopeOf = (
  agents: AgentRegistryShape,
  requested: readonly AgentId[] | undefined
): readonly AgentId[] => {
  const wanted = new Set(requested ?? AGENT_IDS);
  return AGENT_IDS.filter(
    (id) => wanted.has(id) && agents.roots(id).supported && PARSERS[id]
  );
};

/** The window, resolved from the injected clock. */
const windowOf = (now: number, days: number): StatsWindow => ({
  days,
  fromIso: new Date(now - days * MS_PER_DAY).toISOString(),
  toIso: new Date(now).toISOString(),
});

/** True when a fact's own timestamp sits inside the window, or it has none. */
const inWindow = (fact: StatsFact, fromMs: number, toMs: number): boolean => {
  const ts = fact.row === "session" ? fact.startedAt : fact.ts;
  if (ts === null) {
    return true;
  }
  const t = Date.parse(ts);
  return Number.isNaN(t) || (t >= fromMs && t <= toMs);
};

/** What one transcript contributed to a pass. */
interface FileResult {
  readonly cached: boolean;
  readonly entry: CacheEntry | null;
  readonly facts: readonly StatsFact[];
  readonly skipped: string | null;
}

const skippedResult = (path: string): FileResult => ({
  cached: false,
  entry: null,
  facts: [],
  skipped: path,
});

/** Read one transcript's facts, from the cache when it is unchanged. */
const factsFor = (args: {
  readonly agents: AgentRegistryShape;
  readonly cache: StatsCacheShape;
  readonly force: boolean;
  readonly fs: FileSystem.FileSystem;
  readonly index: ReadonlyMap<string, CacheEntry>;
  readonly redact: Redactor;
  readonly ref: TranscriptRef;
  readonly useCache: boolean;
}): Effect.Effect<FileResult> => {
  const { agents, cache, force, fs, index, redact, ref, useCache } = args;
  return Effect.gen(function* () {
    const stat = yield* statOf({ fs, ref });
    const entry = index.get(ref.path);
    if (useCache && !force && entry && isFresh({ entry, ref, stat })) {
      const cached = yield* cache
        .read(entry)
        .pipe(Effect.orElseSucceed(() => null));
      if (cached) {
        return { facts: cached, entry, cached: true, skipped: null };
      }
    }
    const parser = PARSERS[ref.agent];
    if (!parser) {
      return skippedResult(ref.path);
    }
    const loaded = yield* agents
      .loadTranscript({ agent: ref.agent, ref })
      .pipe(Effect.orElseSucceed(() => null));
    if (loaded === null) {
      return skippedResult(ref.path);
    }
    const parsed = parser.parseSession({
      text: loaded.text,
      path: ref.path,
      sessionId: ref.id,
      slug: ref.slug,
    });
    // One project key across agents: Codex has no slug, so its cwd is encoded
    // the same way Claude's project dir already is.
    const project =
      ref.slug === "" ? agents.encodeSlug(parsed.cwd ?? "") : ref.slug;
    const facts = extractFacts({ ref, parsed, project, redact });
    const written = useCache
      ? yield* cache
          .write({ ref, stat, facts })
          .pipe(Effect.orElseSucceed(() => null))
      : null;
    return { facts, entry: written, cached: false, skipped: null };
  });
};

/** What one pass over the corpus produced, before the detectors are asked. */
interface PassResult {
  readonly cached: number;
  readonly extracted: number;
  readonly facts: number;
  readonly skipped: readonly string[];
  readonly transcripts: number;
}

/** Walk the corpus once, folding every in-window fact into `sinks`. */
const runPass = (args: {
  readonly agents: AgentRegistryShape;
  readonly cache: StatsCacheShape;
  readonly force: boolean;
  readonly fs: FileSystem.FileSystem;
  /** Narrow the walk to some transcripts. Default: every one the scope finds. */
  readonly only?: (ref: TranscriptRef) => boolean;
  readonly redact: Redactor;
  readonly scope: readonly AgentId[];
  readonly sinks: readonly FactSink[];
  readonly useCache: boolean;
  readonly window: StatsWindow;
}): Effect.Effect<PassResult, StatsScanError> => {
  const {
    agents,
    cache,
    force,
    fs,
    only,
    redact,
    scope,
    sinks,
    useCache,
    window,
  } = args;
  return Effect.gen(function* () {
    const all = yield* listCorpus({ fs, agents, scope });
    const refs = only ? all.filter(only) : all;
    const index = useCache
      ? yield* cache.load()
      : new Map<string, CacheEntry>();
    const fromMs = Date.parse(window.fromIso);
    const toMs = Date.parse(window.toIso);
    const entries: CacheEntry[] = [];
    const skipped: string[] = [];
    let extracted = 0;
    let cached = 0;
    let facts = 0;

    yield* foldCorpus({
      refs,
      perFile: (ref) =>
        factsFor({ agents, cache, force, fs, index, redact, ref, useCache }),
      onFile: (result) => {
        if (result.skipped !== null) {
          skipped.push(result.skipped);
          return;
        }
        if (result.entry) {
          entries.push(result.entry);
        }
        if (result.cached) {
          cached += 1;
        } else {
          extracted += 1;
        }
        for (const fact of result.facts) {
          if (!inWindow(fact, fromMs, toMs)) {
            continue;
          }
          facts += 1;
          for (const sink of sinks) {
            sink.add(fact);
          }
        }
      },
    });

    if (useCache) {
      // A narrowed pass saw one transcript, so it may only add to the index.
      // Replacing it would drop every other shard and force a full re-parse.
      const merged = only
        ? [
            ...new Map([
              ...index,
              ...entries.map((e) => [e.path, e] as const),
            ]).values(),
          ]
        : entries;
      yield* cache.commit(merged).pipe(Effect.orElseSucceed(() => undefined));
    }
    return {
      transcripts: refs.length,
      extracted,
      cached,
      facts,
      skipped: uniqSorted(skipped),
    };
  }).pipe(
    Effect.catchAllDefect((e) =>
      Effect.fail(new StatsScanError({ reason: String(e) }))
    )
  );
};

/** Everything a report pass folds into. */
interface Detectors {
  readonly clusters: ReturnType<typeof makeClusterTable>;
  readonly corpus: CorpusTotals;
  readonly fails: ReturnType<typeof makeFailTable>;
  readonly panels: ReturnType<typeof makeCorpusPanels>;
  readonly time: ReturnType<typeof makeTimeTable>;
  readonly waiting: ReturnType<typeof makeWaitPanel>;
}

const buildDetectors = (opts: AccumulatorOptions): Detectors => ({
  clusters: makeClusterTable(opts),
  corpus: makeCorpusTotals(),
  fails: makeFailTable(opts),
  panels: makeCorpusPanels(opts),
  time: makeTimeTable(opts),
  waiting: makeWaitPanel(opts),
});

const sinksOf = (d: Detectors): readonly FactSink[] => [
  d.corpus,
  d.fails,
  d.time,
  d.waiting,
  d.clusters,
  d.panels,
];

/** The accumulator a drill key belongs to. */
const sinkForTable = (
  d: Detectors,
  table: DrillTable
): FactAccumulator<unknown> => {
  switch (table) {
    case "fail":
      return d.fails;
    case "time":
      return d.time;
    case "cluster":
      return d.clusters;
    default:
      return d.waiting;
  }
};

/** Everything one pass needs, resolved from the request and the clock. */
interface Prepared {
  readonly caveats: string[];
  readonly detectors: Detectors;
  readonly generatedAt: number;
  readonly opts: AccumulatorOptions;
  readonly redact: Redactor;
  readonly result: PassResult;
  readonly window: StatsWindow;
}

/** Build the finish context. Corpus totals are computed first, by construction:
 * every share in the report divides by a field the detectors read from here. */
const contextOf = (prepared: Prepared): FinishContext => ({
  corpus: prepared.detectors.corpus.finish(),
  window: prepared.window,
  redact: prepared.redact,
  limit: prepared.opts.limit,
  sampleCap: prepared.opts.sampleCap,
  caveat: (note) => {
    prepared.caveats.push(note);
  },
});

const makeService = (args: {
  readonly agents: AgentRegistryShape;
  readonly cache: StatsCacheShape;
  readonly fs: FileSystem.FileSystem;
}): StatsServiceShape => {
  const { agents, cache, fs } = args;

  /** One pass, shared by `report` and `drill`. */
  const prepare = (
    req: ReportRequest,
    drillTarget: DrillTarget | null
  ): Effect.Effect<Prepared, StatsScanError> =>
    Effect.gen(function* () {
      const generatedAt = yield* Clock.currentTimeMillis;
      const window = windowOf(generatedAt, req.days ?? DEFAULT_DAYS);
      const redactOn = req.redact !== false;
      const redact = makeRedactor({ enabled: redactOn });
      const opts: AccumulatorOptions = {
        redact,
        limit: req.limit ?? DEFAULT_ROW_LIMIT,
        sampleCap: DEFAULT_SAMPLE_CAP,
        drill: drillTarget,
      };
      const detectors = yield* Effect.try({
        try: () => buildDetectors(opts),
        catch: (e) => new StatsScanError({ reason: String(e) }),
      });
      const result = yield* runPass({
        agents,
        cache,
        force: req.force === true,
        fs,
        redact,
        scope: scopeOf(agents, req.agents),
        sinks: sinksOf(detectors),
        // Raw text is never written down: an unredacted pass skips the cache.
        useCache: redactOn,
        window,
      });
      return {
        caveats: [...BASE_CAVEATS],
        detectors,
        generatedAt,
        opts,
        redact,
        result,
        window,
      };
    });

  const report: StatsServiceShape["report"] = (req) =>
    prepare(req, null).pipe(
      Effect.flatMap((prepared) =>
        Effect.try({
          try: (): StatsReport => {
            const ctx = contextOf(prepared);
            const { detectors } = prepared;
            const panels = detectors.panels.finish(ctx);
            return {
              schemaVersion: STATS_SCHEMA_VERSION,
              generatedAt: prepared.generatedAt,
              window: prepared.window,
              detectorVersion: DETECTOR_VERSION,
              corpus: ctx.corpus,
              fails: detectors.fails.finish(ctx),
              time: detectors.time.finish(ctx),
              waiting: detectors.waiting.finish(ctx),
              clusters: detectors.clusters.finish(ctx),
              context: panels.context,
              fanout: panels.fanout,
              web: panels.web,
              cost: panels.cost,
              skipped: prepared.result.skipped,
              caveats: prepared.caveats,
            };
          },
          catch: (e) => new StatsScanError({ reason: String(e) }),
        })
      ),
      Effect.withSpan("Stats.report")
    );

  const drill: StatsServiceShape["drill"] = (req) =>
    prepare(req, { table: req.table, key: req.key }).pipe(
      Effect.flatMap((prepared) =>
        Effect.try({
          try: () =>
            sinkForTable(prepared.detectors, req.table).drill(
              contextOf(prepared)
            ),
          catch: (e) => new StatsScanError({ reason: String(e) }),
        }).pipe(
          Effect.flatMap((hits) => {
            if (hits.length === 0) {
              return Effect.fail(
                new StatsRowNotFoundError({ key: req.key, table: req.table })
              );
            }
            const offset = Math.max(0, Math.trunc(req.offset ?? 0));
            const page = Math.max(
              1,
              Math.trunc(req.pageSize ?? DEFAULT_DRILL_PAGE)
            );
            return Effect.succeed({
              schemaVersion: STATS_SCHEMA_VERSION,
              table: req.table,
              key: req.key,
              total: hits.length,
              offset,
              hits: hits.slice(offset, offset + page),
            } satisfies DrillResult);
          })
        )
      ),
      Effect.withSpan("Stats.drill", { attributes: { table: req.table } })
    );

  const markers: StatsServiceShape["markers"] = (req) =>
    Effect.gen(function* () {
      const now = yield* Clock.currentTimeMillis;
      const redact = makeRedactor({ enabled: true });
      const collector = makeMarkerCollector({ redact, sid: req.sid });
      yield* runPass({
        agents,
        cache,
        force: req.force === true,
        fs,
        // One session's transcript, not the corpus: an unknown id marks nothing.
        only: (ref) => ref.id === req.sid,
        redact,
        scope: scopeOf(agents, req.agents),
        sinks: [collector],
        useCache: true,
        window: windowOf(now, ALL_DAYS),
      });
      return {
        schemaVersion: STATS_SCHEMA_VERSION,
        sid: req.sid,
        detectorVersion: DETECTOR_VERSION,
        markers: collector.finish(),
      } satisfies SessionMarkers;
    }).pipe(Effect.withSpan("Stats.markers"));

  const refresh: StatsServiceShape["refresh"] = (req) =>
    Effect.gen(function* () {
      const now = yield* Clock.currentTimeMillis;
      if (req.force === true) {
        yield* cache.clear().pipe(Effect.orElseSucceed(() => undefined));
      }
      const result = yield* runPass({
        agents,
        cache,
        force: req.force === true,
        fs,
        redact: makeRedactor({ enabled: true }),
        scope: scopeOf(agents, req.agents),
        sinks: [],
        useCache: true,
        // The cache holds every fact; the report window is a read-time filter.
        window: windowOf(now, ALL_DAYS),
      });
      return {
        transcripts: result.transcripts,
        extracted: result.extracted,
        cached: result.cached,
        skipped: result.skipped,
        facts: result.facts,
      } satisfies RefreshSummary;
    }).pipe(Effect.withSpan("Stats.refresh"));

  return { drill, markers, refresh, report };
};

/** Live layer: depends on AgentRegistry, the platform FileSystem and the cache. */
export const StatsServiceLive = Layer.effect(
  StatsService,
  Effect.gen(function* () {
    const fs = yield* Fs.FileSystem;
    const agents = yield* AgentRegistry;
    const cache = yield* StatsCache;
    return makeService({ agents, cache, fs });
  })
);
