/** What a cold start leads with, decided once for every surface.
 *
 * A finding is a ranked row that carries a change someone can make. Which rows
 * qualify, what order they take and what each one says live here; how a number
 * is spelled does not. A terminal pads and clips, a browser reads a locale, so
 * the caller passes its own primitives and this module composes the sentences.
 *
 * The two tables are never blended into one score: a failure count and a second
 * are different units, and the seconds are carried by a thin tail of long runs.
 * Findings rank in effort tiers instead, and inside a tier by the ranking the
 * table already applied in core.
 */
import type { FailRow, SampleRef, StatsReport, TimeRow } from "./schema";

/** How the calling surface spells a number. */
export interface FindingFormat {
  /** A count with thousands separators. */
  readonly count: (value: number) => string;
  /** Seconds as a duration, never a bare float. */
  readonly hms: (sec: number) => string;
  /** A 0-1 share as a percentage. */
  readonly pct: (share: number) => string;
}

/** One flagged finding: a row that names a change, plus the route back. */
export interface Finding {
  readonly detector: string | null;
  /** What it costs, in the unit its own table ranks by. */
  readonly impact: string;
  readonly key: string;
  readonly note: string;
  /** Sessions this row came from; the first is one action away. */
  readonly samples: readonly SampleRef[];
  /** How wide it reaches: the second number, in that table's own columns. */
  readonly spread: string;
  readonly table: "fail" | "time";
  readonly title: string;
}

/** The spread line of a fail row, shared by the band and the note strip. */
export const failSpread = (row: FailRow, f: FindingFormat): string =>
  `${f.count(row.rows)} calls · ${f.count(row.projects)} projects · ${f.count(row.flagged)} flagged`;

/**
 * The spread line of a time row. A share of nothing is not zero percent, so a
 * corpus with no shell seconds says what it is instead of printing `0.00%`.
 */
export const timeSpread = (
  row: TimeRow,
  bashSec: number,
  f: FindingFormat
): string =>
  bashSec > 0
    ? `${f.pct(row.share)} of shell time · ${f.count(row.runs)} runs`
    : `not measured · ${f.count(row.runs)} runs`;

/**
 * What a time row can say for itself, from its own columns. `TimeRow` carries no
 * note — the fail table is where a named detector writes one — so this states
 * only what the numbers already measured, never advice nobody observed.
 */
export const timeNote = (row: TimeRow, f: FindingFormat): string | null => {
  if (row.cappedRuns > 0) {
    return `${f.count(row.cappedRuns)} of ${f.count(row.runs)} runs returned at a harness ceiling (${f.hms(row.cappedSec)} of ${f.hms(row.totalSec)}): these seconds are a floor`;
  }
  if (row.bgRuns > 0) {
    return `${f.count(row.bgRuns)} runs were backgrounded and charged only the seconds they took, which understates the wait they started`;
  }
  return null;
};

const failFinding = (row: FailRow, f: FindingFormat): Finding | null =>
  row.note === null
    ? null
    : {
        detector: row.detector,
        impact: `${f.count(row.sessions)} sessions`,
        key: row.key,
        note: row.note,
        samples: row.samples,
        spread: failSpread(row, f),
        table: "fail",
        title: row.label,
      };

const timeFinding = (
  row: TimeRow,
  bashSec: number,
  f: FindingFormat
): Finding | null => {
  const note = timeNote(row, f);
  return note === null
    ? null
    : {
        detector: null,
        impact: f.hms(row.totalSec),
        key: row.key,
        note,
        samples: row.samples,
        spread: timeSpread(row, bashSec, f),
        table: "time",
        title: row.label,
      };
};

/**
 * The cold-start list: rows that name a change, cheapest change first.
 *
 * Tier 1 is a fail row whose `detector` names the rule to change — one line of
 * shell, measured. Tier 2 is a fail row with a measured note and no named rule.
 * Tier 3 is a time row, which can only say how its seconds were measured.
 */
export const findingsOf = (
  report: StatsReport,
  f: FindingFormat
): readonly Finding[] => {
  const fails = report.fails.flatMap((row) => failFinding(row, f) ?? []);
  return [
    ...fails.filter((one) => one.detector !== null),
    ...fails.filter((one) => one.detector === null),
    ...report.time.flatMap(
      (row) => timeFinding(row, report.corpus.bashSec, f) ?? []
    ),
  ];
};
