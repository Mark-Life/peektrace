/**
 * `stats.usage`: one pass over the cached facts, folded into the usage sink.
 *
 * The pass is widened past the requested days so the first bucket and the
 * first block are whole; the sink trims afterwards. In block mode, when the
 * widened pass still cuts a block chain, a second pass reads full history.
 */
import { Clock, Effect, Schema } from "effect";
import type { AgentId } from "../agent-id";
import type { FactSink } from "./accumulate";
import { DAY_MS, makeUsageSink } from "./detectors/usage";
import { StatsScanError } from "./errors";
import type { StatsWindow } from "./schema";
import {
  DEFAULT_USAGE_DAYS,
  MAX_USAGE_DAYS,
  USAGE_SCHEMA_VERSION,
  type UsageBy,
  UsageDays,
  type UsageReport,
} from "./usage-schema";

/** Longest month, plus one day of zone offset. */
const MONTH_WIDEN_DAYS = 32;

/** Time before the cutoff the pass also reads, so the first bucket is whole.
 * Day: the first local day can start up to a day early (zone offset, DST).
 * Month: the first month can start up to 31 days early, plus that day.
 * Block: room to find a gap over 5 hours, which resets the block chain. */
const WIDEN_MS: Readonly<Record<UsageBy, number>> = {
  day: DAY_MS,
  month: MONTH_WIDEN_DAYS * DAY_MS,
  block: DAY_MS,
};

const isUsageDays = Schema.is(UsageDays);

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
type UsagePass = (args: {
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
      const days = req.days ?? DEFAULT_USAGE_DAYS[req.by];
      if (!isUsageDays(days)) {
        return yield* new StatsScanError({
          reason: `days must be a whole number from 1 to ${MAX_USAGE_DAYS}: ${days}`,
        });
      }
      yield* Effect.try({
        try: () => new Intl.DateTimeFormat("en-US", { timeZone: tz }),
        catch: () => new StatsScanError({ reason: `unknown time zone: ${tz}` }),
      });
      const fromMs = now - days * DAY_MS;
      const passFromMs = fromMs - WIDEN_MS[req.by];
      const run = (opts: { readonly from?: number; readonly force: boolean }) =>
        Effect.gen(function* () {
          const sink = makeUsageSink({
            by: req.by,
            days,
            fromMs: opts.from,
            nowMs: now,
            tz,
          });
          yield* args.pass({
            agents: req.agents,
            force: opts.force,
            sinks: [sink],
            window: windowOf(opts.from ?? 0, now, days),
          });
          return sink.finish();
        });
      const first = yield* run({
        from: passFromMs,
        force: req.force === true,
      });
      // The first pass already refreshed the cache, so the second never forces.
      const out = first.chainCut ? yield* run({ force: false }) : first;
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
