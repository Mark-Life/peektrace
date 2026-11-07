/**
 * Usage over time: tokens and estimated cost per day, month or 5-hour block.
 *
 * Reads `usage` rows only, one per model call. Day and month buckets stream
 * into a map keyed by bucket and model, so memory is bounded by buckets, not
 * calls. Blocks need the calls in time order, so block mode keeps one compact
 * record per Claude call and folds them in `finish`.
 *
 * Block rules follow ccusage: a block opens at the UTC hour of its first call
 * and lasts 5 hours; a call past that opens the next block.
 *
 * A Claude resume file copies the history it resumes, request ids included, so
 * a Claude request id counts once across transcripts. Other agents' ids are not
 * known to be globally unique and dedup within their own transcript only.
 */
import type { Types } from "effect";
import type { FactSink } from "../accumulate";
import { round, uniqSorted } from "../rank";
import type { StatsFact, UsageFact } from "../schema";
import type {
  UsageBlock,
  UsageBy,
  UsageRow,
  UsageSkipped,
  UsageTotals,
} from "../usage-schema";
import { estimateCost, FALLBACK_FAMILY, priceFor, pricingEcho } from "./corpus";

const HOUR_MS = 3_600_000;
const DAY_MS = 86_400_000;
/** Length of a billing block, in hours. */
const BLOCK_HOURS = 5;
const BLOCK_MS = BLOCK_HOURS * HOUR_MS;

/** Every IANA offset is a multiple of 15 minutes, so a day key is constant
 * across one 15-minute slot. An hour slot would be wrong at +5:30 or +5:45. */
const SLOT_MS = 900_000;

/** Request ids a parser mints from the line number: unique per file only. */
const LINE_ID = /^line-\d+$/;

/** Length of `YYYY-MM`. */
const MONTH_LEN = 7;

/** Length of `YYYY-MM-DD`. */
const DAY_LEN = 10;

/** The token columns of a usage row. */
type Tokens = Pick<
  UsageFact,
  "cacheCreationTokens" | "cacheReadTokens" | "inputTokens" | "outputTokens"
>;

/** Tokens and call count folded under one bucket and model. */
interface Agg extends Types.Mutable<Tokens> {
  turns: number;
}

/** One Claude call, kept for the block fold. */
interface BlockCall {
  readonly model: string;
  readonly ms: number;
  readonly tokens: Tokens;
}

const noAgg = (): Agg => ({
  turns: 0,
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
});

const addInto = (agg: Agg, tokens: Tokens, turns = 1): void => {
  agg.turns += turns;
  agg.inputTokens += tokens.inputTokens;
  agg.outputTokens += tokens.outputTokens;
  agg.cacheReadTokens += tokens.cacheReadTokens;
  agg.cacheCreationTokens += tokens.cacheCreationTokens;
};

const tokensOf = (fact: Tokens): Tokens => ({
  inputTokens: fact.inputTokens,
  outputTokens: fact.outputTokens,
  cacheReadTokens: fact.cacheReadTokens,
  cacheCreationTokens: fact.cacheCreationTokens,
});

const totalOf = (t: Tokens): number =>
  t.inputTokens + t.outputTokens + t.cacheReadTokens + t.cacheCreationTokens;

/** The key two copies of one model call share. */
export const dedupKeyOf = (fact: Pick<UsageFact, "agent" | "req" | "sid">) =>
  fact.agent === "claude" && !LINE_ID.test(fact.req)
    ? fact.req
    : `${fact.sid}\0${fact.req}`;

/**
 * `YYYY-MM-DD` of an epoch ms in `tz`, memoized per 15-minute slot. Throws a
 * `RangeError` on an unknown zone, at build time.
 */
export const dayKeyOf = (tz: string) => {
  const fmt = new Intl.DateTimeFormat("en-CA", {
    timeZone: tz,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
  const memo = new Map<number, string>();
  return (ms: number) => {
    const slot = Math.floor(ms / SLOT_MS);
    const hit = memo.get(slot);
    if (hit !== undefined) {
      return hit;
    }
    const parts = fmt.formatToParts(slot * SLOT_MS);
    const part = (type: Intl.DateTimeFormatPartTypes) =>
      parts.find((p) => p.type === type)?.value ?? "";
    const key = `${part("year")}-${part("month")}-${part("day")}`;
    memo.set(slot, key);
    return key;
  };
};

/** The `YYYY-MM-DD` that is `n` calendar days before `day`. Steps on the
 * date itself, so a DST change in between does not shift it. */
const daysBefore = (day: string, n: number) => {
  const [y = 0, m = 1, d = 1] = day.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d - n)).toISOString().slice(0, DAY_LEN);
};

/** One folded block, before pricing. */
interface BlockOf {
  readonly active: boolean;
  readonly calls: readonly BlockCall[];
  readonly endMs: number;
  readonly lastMs: number;
  readonly startMs: number;
}

/** Fold calls into 5-hour blocks. A gap over 5 hours always overruns the
 * open block too, so one check covers both of ccusage's rules. */
export const blocksOf = (calls: readonly BlockCall[], nowMs: number) => {
  const sorted = [...calls].sort((a, b) => a.ms - b.ms);
  const open: { startMs: number; lastMs: number; calls: BlockCall[] }[] = [];
  for (const call of sorted) {
    const cur = open.at(-1);
    if (cur === undefined || call.ms - cur.startMs > BLOCK_MS) {
      const startMs = Math.floor(call.ms / HOUR_MS) * HOUR_MS;
      open.push({ startMs, lastMs: call.ms, calls: [call] });
      continue;
    }
    cur.lastMs = call.ms;
    cur.calls.push(call);
  }
  return open.map(
    (b): BlockOf => ({
      ...b,
      endMs: b.startMs + BLOCK_MS,
      active: nowMs < b.startMs + BLOCK_MS,
    })
  );
};

/** What the sink is built with. */
export interface UsageSinkOptions {
  readonly by: UsageBy;
  /** Buckets kept: days back from `nowMs`, the first bucket kept whole.
   * A whole number from 1; the caller checks it. */
  readonly days: number;
  readonly nowMs: number;
  /** IANA zone for day and month keys. */
  readonly tz: string;
}

/** What the sink produces. */
interface UsageSinkResult {
  readonly caveats: readonly string[];
  readonly rows: readonly UsageRow[];
  readonly skipped: UsageSkipped;
  readonly totals: UsageTotals;
  readonly unpricedModels: readonly string[];
}

/** The usage sink. Throws a `RangeError` on an unknown `tz`. */
export const makeUsageSink = (opts: UsageSinkOptions) => {
  const { by, days, nowMs, tz } = opts;
  const dayKey = dayKeyOf(tz);
  const cutoffDay = daysBefore(dayKey(nowMs), days - 1);
  const cutoffMs = nowMs - days * DAY_MS;
  const bucketKeyOf = (ms: number) =>
    by === "month" ? dayKey(ms).slice(0, MONTH_LEN) : dayKey(ms);
  const cutoffKey = by === "month" ? cutoffDay.slice(0, MONTH_LEN) : cutoffDay;

  const buckets = new Map<string, Map<string, Agg>>();
  const calls: BlockCall[] = [];
  const seen = new Set<string>();
  const skipped = {
    noTimestamp: 0,
    zeroTokens: 0,
    duplicates: 0,
    otherAgents: 0,
  };

  const fold = (
    target: Map<string, Agg>,
    model: string,
    tokens: Tokens
  ): void => {
    const agg = target.get(model) ?? noAgg();
    addInto(agg, tokens);
    target.set(model, agg);
  };

  /** Epoch ms of a call the view counts, or null after counting the skip. */
  const admit = (fact: UsageFact): number | null => {
    if (totalOf(fact) === 0) {
      skipped.zeroTokens += 1;
      return null;
    }
    const ms = fact.ts === null ? Number.NaN : Date.parse(fact.ts);
    if (Number.isNaN(ms)) {
      skipped.noTimestamp += 1;
      return null;
    }
    if (by === "block" && fact.agent !== "claude") {
      skipped.otherAgents += 1;
      return null;
    }
    return ms;
  };

  /** True for a second copy of a call already counted. */
  const isDuplicate = (fact: UsageFact): boolean => {
    const key = dedupKeyOf(fact);
    if (seen.has(key)) {
      skipped.duplicates += 1;
      return true;
    }
    seen.add(key);
    return false;
  };

  const add = (fact: StatsFact): void => {
    if (fact.row !== "usage") {
      return;
    }
    const ms = admit(fact);
    if (ms === null) {
      return;
    }
    if (by === "block") {
      if (!isDuplicate(fact)) {
        calls.push({ ms, model: fact.model, tokens: tokensOf(fact) });
      }
      return;
    }
    const key = bucketKeyOf(ms);
    if (key < cutoffKey || isDuplicate(fact)) {
      return;
    }
    const bucket = buckets.get(key) ?? new Map<string, Agg>();
    fold(bucket, fact.model, fact);
    buckets.set(key, bucket);
  };

  const totalsOf = (byModel: ReadonlyMap<string, Agg>): UsageTotals => {
    const sum = noAgg();
    let usd = 0;
    for (const [model, agg] of byModel) {
      addInto(sum, agg, agg.turns);
      usd += estimateCost(agg, priceFor(model).rates);
    }
    return {
      ...sum,
      totalTokens: totalOf(sum),
      usd: round(usd),
      models: uniqSorted(byModel.keys()),
    };
  };

  const blockRows = () =>
    blocksOf(calls, nowMs)
      .filter((b) => b.endMs > cutoffMs)
      .map((b) => {
        const byModel = new Map<string, Agg>();
        for (const call of b.calls) {
          fold(byModel, call.model, call.tokens);
        }
        const block: UsageBlock = {
          startIso: new Date(b.startMs).toISOString(),
          endIso: new Date(b.endMs).toISOString(),
          lastIso: new Date(b.lastMs).toISOString(),
          active: b.active,
        };
        return { key: block.startIso, byModel, block };
      });

  const bucketRows = () =>
    [...buckets.entries()]
      .sort(([a], [b]) => (a < b ? -1 : 1))
      .map(([key, byModel]) => ({ key, byModel, block: null }));

  const finish = (): UsageSinkResult => {
    const built = by === "block" ? blockRows() : bucketRows();
    const all = new Map<string, Agg>();
    for (const row of built) {
      for (const [model, agg] of row.byModel) {
        const into = all.get(model) ?? noAgg();
        addInto(into, agg, agg.turns);
        all.set(model, into);
      }
    }
    const unpricedModels = uniqSorted(
      [...all.keys()].filter((model) => !priceFor(model).priced)
    );
    const caveats = [
      `Cost is an ESTIMATE at published list prices per million tokens (input/output/cacheWrite/cacheRead USD): ${pricingEcho()}`,
    ];
    if (unpricedModels.length > 0) {
      caveats.push(
        `Cost: ${unpricedModels.length} model ids have no rate card and are estimated at ${FALLBACK_FAMILY} rates, so their cost is a guess — ${unpricedModels.join(", ")}`
      );
    }
    if (skipped.duplicates > 0) {
      caveats.push(
        `${skipped.duplicates} model calls repeat a request id already seen and count once, so totals can sit below the report's cost panel`
      );
    }
    if (skipped.noTimestamp > 0) {
      caveats.push(
        `${skipped.noTimestamp} model calls carry no timestamp and are left out`
      );
    }
    if (by === "block") {
      caveats.push(
        "a block is the 5 hours from the UTC hour of its first call; a later call opens the next block"
      );
    }
    if (skipped.otherAgents > 0) {
      caveats.push(
        `blocks cover Claude turns only: ${skipped.otherAgents} calls from other agents are left out`
      );
    }
    return {
      rows: built.map(({ key, byModel, block }) => ({
        key,
        ...totalsOf(byModel),
        block,
      })),
      totals: totalsOf(all),
      unpricedModels,
      skipped: { ...skipped },
      caveats,
    };
  };

  return { add, finish } satisfies FactSink & {
    readonly finish: () => UsageSinkResult;
  };
};
