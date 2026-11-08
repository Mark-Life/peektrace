/**
 * The usage-over-time wire contract: tokens and estimated cost per day, month
 * or 5-hour block. Its own artifact with its own `schemaVersion`, so adding it
 * leaves `StatsReport` untouched.
 */
import { Schema } from "effect";
import { StatsWindow } from "./schema";

export const USAGE_SCHEMA_VERSION = "peektrace-stats-usage/v1";

/** How the usage view buckets model calls. */
export const UsageBy = Schema.Literal("day", "month", "block");
export type UsageBy = typeof UsageBy.Type;

/** Days a view covers when the caller names none. */
export const DEFAULT_USAGE_DAYS: Readonly<Record<UsageBy, number>> = {
  day: 30,
  month: 365,
  block: 7,
};

/** Most days a view may cover: 100 years. */
export const MAX_USAGE_DAYS = 36_500;

/** A whole number of days, 1 to `MAX_USAGE_DAYS`. */
export const UsageDays = Schema.Int.pipe(Schema.between(1, MAX_USAGE_DAYS));

/** A 5-hour billing window, opened at the UTC hour of its first call. */
export const UsageBlock = Schema.Struct({
  startIso: Schema.String,
  /** `startIso` plus 5 hours. */
  endIso: Schema.String,
  /** Time of the block's last call. */
  lastIso: Schema.String,
  /** The window is still open and its last call is under 5 hours old. */
  active: Schema.Boolean,
});
export type UsageBlock = typeof UsageBlock.Type;

const usageTotalsFields = {
  /** Model calls counted, after dedup. */
  turns: Schema.Number,
  inputTokens: Schema.Number,
  outputTokens: Schema.Number,
  cacheReadTokens: Schema.Number,
  cacheCreationTokens: Schema.Number,
  totalTokens: Schema.Number,
  /** Estimated USD at API list prices; unpriced models add nothing. */
  usd: Schema.Number,
  models: Schema.Array(Schema.String),
};

/** The totals line: every row column but the bucket identity. */
export const UsageTotals = Schema.Struct(usageTotalsFields);
export type UsageTotals = typeof UsageTotals.Type;

/** One bucket. */
export const UsageRow = Schema.Struct({
  /** `YYYY-MM-DD`, `YYYY-MM`, or the block's `startIso`. */
  key: Schema.String,
  ...usageTotalsFields,
  /** Set in block mode only. */
  block: Schema.NullOr(UsageBlock),
});
export type UsageRow = typeof UsageRow.Type;

/** Model calls the view left out, by reason. */
export const UsageSkipped = Schema.Struct({
  /** The transcript records no time for the call. */
  noTimestamp: Schema.Number,
  /** All four token counts are zero, e.g. a synthetic error turn. */
  zeroTokens: Schema.Number,
  /** Repeated request ids: the call is counted once. */
  duplicates: Schema.Number,
  /** Block mode only: calls by agents other than Claude. */
  otherAgents: Schema.Number,
});
export type UsageSkipped = typeof UsageSkipped.Type;

/** What `peektrace stats usage --json` prints. */
export const UsageReport = Schema.Struct({
  schemaVersion: Schema.Literal(USAGE_SCHEMA_VERSION),
  /** Epoch ms from the injected clock. */
  generatedAt: Schema.Number,
  by: UsageBy,
  /** IANA zone the day and month keys are cut in. */
  tz: Schema.String,
  window: StatsWindow,
  /** Ascending by key. The first bucket is whole, so it may start before `window.fromIso`. */
  rows: Schema.Array(UsageRow),
  totals: UsageTotals,
  /** Model ids with no rate card: they add no cost. */
  unpricedModels: Schema.Array(Schema.String),
  skipped: UsageSkipped,
  caveats: Schema.Array(Schema.String),
});
export type UsageReport = typeof UsageReport.Type;
