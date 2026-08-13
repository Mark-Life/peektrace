/** Formatters for the stats screen, and the findings order bound to them.
 *
 * Which rows lead a cold start, and what each one says, is core's answer
 * (`services/stats/findings`); this file only supplies how a number is spelled
 * in a browser. The terminal binds the same functions to its own primitives, so
 * neither surface can drift from the other by editing a sentence.
 */
import {
  findingsOf as coreFindingsOf,
  timeNote as coreTimeNote,
  type FindingFormat,
} from "@workspace/core/services/stats/findings";

export type { Finding } from "@workspace/core/services/stats/findings";

import type {
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";

const SEC_PER_MIN = 60;
const SEC_PER_HOUR = 3600;
const PERCENT = 100;

/** Decimals a share keeps once it is a percentage. */
const PCT_DIGITS = 2;

/** `4h12m`, `3m07s`, `0.42s` — never a bare float of 15,000 seconds. */
export const hms = (sec: number): string => {
  if (sec < SEC_PER_MIN) {
    return `${sec.toFixed(sec < 10 ? 2 : 0)}s`;
  }
  if (sec < SEC_PER_HOUR) {
    const m = Math.floor(sec / SEC_PER_MIN);
    return `${m}m${String(Math.round(sec - m * SEC_PER_MIN)).padStart(2, "0")}s`;
  }
  const h = Math.floor(sec / SEC_PER_HOUR);
  return `${h}h${String(Math.floor((sec - h * SEC_PER_HOUR) / SEC_PER_MIN)).padStart(2, "0")}m`;
};

/** A 0-1 share as a percentage string. */
export const pct = (share: number): string =>
  `${(share * PERCENT).toFixed(PCT_DIGITS)}%`;

/** Thousands separators, one call site for every count in the screen. */
export const n = (value: number): string => value.toLocaleString();

/** A short local timestamp for the report header. */
export const at = (epochMs: number): string =>
  new Date(epochMs).toLocaleString();

/** How this surface spells a number, for the sentences core composes. */
const FORMAT: FindingFormat = { count: n, hms, pct };

/** What a time row can say for itself, from its own columns. */
export const timeNote = (row: TimeRow): string | null =>
  coreTimeNote(row, FORMAT);

/** How many lines held this job but were charged to a higher-priority one. */
export const coPresentNote = (row: TimeRow): string =>
  `${n(row.coPresentRuns)} lines hold this job and were charged elsewhere`;

/** The cold-start list: rows that carry a fix, cheapest fix first. */
export const findingsOf = (report: StatsReport) =>
  coreFindingsOf(report, FORMAT);
