/**
 * `stats.usage`: one pass over the cached facts, folded into the usage sink.
 *
 * The pass is widened past the requested days so the first bucket and the
 * first block are whole; the sink trims by key afterwards.
 */
import { Clock, Effect } from "effect";
import type { AgentId } from "../agent-id";
import type { FactSink } from "./accumulate";
import { makeUsageSink } from "./detectors/usage";
import { StatsScanError } from "./errors";
import type { StatsWindow } from "./schema";
import {
  USAGE_SCHEMA_VERSION,
  type UsageBy,
  type UsageReport,
} from "./usage-schema";

const MS_PER_DAY = 86_400_000;
const MS_PER_HOUR = 3_600_000;

/** Days past the cutoff the pass reads, so the first month is whole. */
const WIDEN_DAYS = 32;

/** Hours past that, so a block that opened before the cutoff is whole. */
const WIDEN_HOURS = 5;

const WIDEN_MS = WIDEN_DAYS * MS_PER_DAY + WIDEN_HOURS * MS_PER_HOUR;

/** Days a view covers when the caller names none. */
export const DEFAULT_USAGE_DAYS: Readonly<Record<UsageBy, number>> = {
  day: 30,
  month: 365,
  block: 7,
};

/** What `usage` accepts. */
export interface UsageRequest {
  readonly agents?: readonly AgentId[];
  readonly by: UsageBy;
  /** Days back from now. Default per `by`: see `DEFAULT_USAGE_DAYS`. */
  readonly days?: number;
  /** Ignore cached shards and re-extract every transcript. */
  readonly force?: boolean;
  /** IANA zone for day and month keys. Default `UTC`. */
  readonly tz?: string;
}

/** One cached pass over the scoped corpus, folding in-window facts into `sinks`. */
export type UsagePass = (args: {
  readonly agents: readonly AgentId[] | undefined;
  readonly force: boolean;
  readonly sinks: readonly FactSink[];
  readonly window: StatsWindow;
}) => Effect.Effect<unknown, StatsScanError>;

const windowOf = (fromMs: number, toMs: number, days: number) => ({
  days,
  fromIso: new Date(fromMs).toISOString(),
  toIso: new Date(toMs).toISOString(),
});

/** Build `usage` over a pass. */
export const makeUsage =
  (args: { readonly pass: UsagePass }) => (req: UsageRequest) =>
    Effect.gen(function* () {
      const now = yield* Clock.currentTimeMillis;
      const tz = req.tz ?? "UTC";
      const days = Math.max(
        1,
        Math.trunc(req.days ?? DEFAULT_USAGE_DAYS[req.by])
      );
      const sink = yield* Effect.try({
        try: () => makeUsageSink({ by: req.by, days, nowMs: now, tz }),
        catch: () => new StatsScanError({ reason: `unknown time zone: ${tz}` }),
      });
      const fromMs = now - days * MS_PER_DAY;
      yield* args.pass({
        agents: req.agents,
        force: req.force === true,
        sinks: [sink],
        window: windowOf(fromMs - WIDEN_MS, now, days),
      });
      const out = sink.finish();
      return {
        schemaVersion: USAGE_SCHEMA_VERSION,
        generatedAt: now,
        by: req.by,
        tz,
        window: windowOf(fromMs, now, days),
        rows: out.rows,
        totals: out.totals,
        unpricedModels: out.unpricedModels,
        skipped: out.skipped,
        caveats: out.caveats,
      } satisfies UsageReport;
    }).pipe(Effect.withSpan("Stats.usage", { attributes: { by: req.by } }));
