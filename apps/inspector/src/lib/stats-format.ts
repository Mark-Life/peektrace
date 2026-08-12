/** Formatters and the findings order for the stats screen.
 *
 * Kept out of the JSX so the ordering rule is unit-testable. Two things settle
 * here. `hms` prints seconds the way the tables read them, and `findingsOf`
 * decides what a cold start leads with.
 *
 * The findings list never blends the two tables into one score: a failure count
 * and a second are different units, and the seconds are dominated by a thin tail
 * of long runs. It ranks in effort tiers instead — a row whose detector names a
 * one-line fix, then a row that only carries a measured note, then the time rows
 * — and inside a tier by the measured impact of that tier's own table.
 */
import type {
  FailRow,
  SampleRef,
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

/**
 * What a time row can say for itself, from its own columns. `TimeRow` carries no
 * `note` — the fail table is where a named detector writes one — so this states
 * only what the numbers already measured, never advice nobody observed.
 */
export const timeNote = (row: TimeRow): string | null => {
  if (row.cappedRuns > 0) {
    return `${n(row.cappedRuns)} of ${n(row.runs)} runs returned at a harness ceiling (${hms(row.cappedSec)} of ${hms(row.totalSec)}): these seconds are a floor`;
  }
  if (row.bgRuns > 0) {
    return `${n(row.bgRuns)} runs were backgrounded and charged only the seconds they took, which understates the wait they started`;
  }
  return null;
};

/** How many lines held this job but were charged to a higher-priority one. */
export const coPresentNote = (row: TimeRow): string =>
  `${n(row.coPresentRuns)} lines hold this job and were charged elsewhere`;

/** One flagged finding: a row that carries a fix, plus a route back. */
export interface Finding {
  readonly detector: string | null;
  /** What it costs, in the unit its own table ranks by. */
  readonly impact: string;
  readonly key: string;
  readonly note: string;
  /** Sessions this row came from; the first is one click away. */
  readonly samples: readonly SampleRef[];
  /** How wide it reaches, for the reader who wants the second number. */
  readonly spread: string;
  readonly table: "fail" | "time";
  readonly title: string;
}

const failFinding = (row: FailRow): Finding | null =>
  row.note === null
    ? null
    : {
        key: row.key,
        table: "fail",
        title: row.label,
        note: row.note,
        detector: row.detector,
        impact: `${n(row.sessions)} sessions`,
        spread: `${n(row.rows)} calls · ${n(row.projects)} projects · ${n(row.flagged)} flagged`,
        samples: row.samples,
      };

const timeFinding = (row: TimeRow): Finding | null => {
  const note = timeNote(row);
  return note === null
    ? null
    : {
        key: row.key,
        table: "time",
        title: row.label,
        note,
        detector: null,
        impact: hms(row.totalSec),
        spread: `${pct(row.share)} of bash · ${n(row.runs)} runs`,
        samples: row.samples,
      };
};

/**
 * The cold-start list: rows that carry a fix, cheapest fix first.
 *
 * Tier 1 is a fail row whose `detector` names the rule to change — one line of
 * shell, measured. Tier 2 is a fail row with a measured note and no named rule.
 * Tier 3 is a time row, which can only say how its seconds were measured. The
 * ranking inside each tier is the table's own, which core already applied.
 */
export const findingsOf = (report: StatsReport): readonly Finding[] => {
  const fails = report.fails.flatMap((row) => failFinding(row) ?? []);
  return [
    ...fails.filter((f) => f.detector !== null),
    ...fails.filter((f) => f.detector === null),
    ...report.time.flatMap((row) => timeFinding(row) ?? []),
  ];
};
