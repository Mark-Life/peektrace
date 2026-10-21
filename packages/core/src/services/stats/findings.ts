/** What a cold start leads with, decided once for every surface.
 *
 * A finding is a ranked row that carries a change someone can make. Which rows
 * qualify, what order they take and what each one says live here; how a number
 * is spelled does not. A terminal pads and clips, a browser reads a locale, so
 * the caller passes its own primitives and this module composes the sentences.
 *
 * The tables are never blended into one score: a failure count, a retry, a
 * second and a token are different units, and the seconds are carried by a
 * thin tail of long runs. Findings rank in effort tiers instead, and inside a
 * tier by the ranking the table already applied in core. Named causes lead:
 * each carries its fix and what the retries it caused cost.
 */
import type {
  CauseRow,
  FailRow,
  RuleCheck,
  SampleRef,
  StatsReport,
  TimeRow,
} from "./schema";

/** What a surface prints where a row has no fix to give. */
export const NO_RULE = "no rule to suggest";

/** How the calling surface spells a number. */
export interface FindingFormat {
  /** A count with thousands separators. */
  readonly count: (value: number) => string;
  /** Seconds as a duration, never a bare float. */
  readonly hms: (sec: number) => string;
  /** A 0-1 share as a percentage. */
  readonly pct: (share: number) => string;
  /** A token count, compact: `950`, `12.3K`, `32.4M`. */
  readonly tokens: (value: number) => string;
}

/** One flagged finding: a row that names a change, plus the route back. */
export interface Finding {
  readonly detector: string | null;
  /** What it costs, in the unit its own table ranks by. */
  readonly impact: string;
  readonly key: string;
  readonly note: string;
  /** The memory line this finding suggests copying, or null. */
  readonly rule: string | null;
  /** Sessions this row came from; the first is one action away. */
  readonly samples: readonly SampleRef[];
  /** How wide it reaches: the second number, in that table's own columns. */
  readonly spread: string;
  readonly table: "cause" | "fail" | "time";
  readonly title: string;
}

/** `main 1 / sub 9`: one count split by who made the calls. */
const splitOf = (
  split: { readonly main: number; readonly nested: number },
  f: FindingFormat
): string => `main ${f.count(split.main)} / sub ${f.count(split.nested)}`;

/** The spread line of a cause row: hits, how they split, and how far. */
export const causeSpread = (row: CauseRow, f: FindingFormat): string => {
  const exit0 = row.hits - row.flagged;
  const hits =
    exit0 > 0
      ? `${f.count(row.hits)} hits (${f.count(exit0)} at exit 0)`
      : `${f.count(row.hits)} hits`;
  const retries = row.failing
    ? ` · ${f.count(row.retrySessions)} sessions with a retry`
    : "";
  return `${hits}${retries} · ${f.count(row.projects)} projects · hits ${splitOf(row.hitsBy, f)}`;
};

/**
 * What a cause cost: the wasted round trips (retry pairs, or read-backs of a
 * persisted output), their seconds and the context they re-read — never summed.
 */
export const causeImpact = (row: CauseRow, f: FindingFormat): string => {
  if (row.pairs === 0) {
    return `${f.count(row.sessions)} sessions`;
  }
  const what = row.failing ? "retries" : "read-backs";
  const tokens =
    row.tokensMeasured > 0
      ? ` · ${f.tokens(row.retryCtxTokens)} ctx tokens re-read`
      : "";
  // No pair with a measured gap is no time, not zero seconds.
  const sec = row.secMeasured === 0 ? "" : ` · ${f.hms(row.retrySec)}`;
  return `${f.count(row.pairs)} ${what} (${splitOf(row.pairsBy, f)})${sec}${tokens}`;
};

/** `~/.claude/CLAUDE.md:100`: where the first matching memory line sits. */
const rulePlace = (rule: RuleCheck): string => {
  const where = rule.matches[0];
  return where === undefined ? "memory" : `${where.path}:${where.line}`;
};

/**
 * What the calls since a rule appeared say about it: still firing, quiet, or
 * not measured yet (no whole day after it appeared).
 */
export const ruleState = (
  rule: RuleCheck
): "firing" | "quiet" | "unmeasured" => {
  if (rule.callsSince.main + rule.callsSince.nested === 0) {
    return "unmeasured";
  }
  return rule.hitsSince.main + rule.hitsSince.nested === 0 ? "quiet" : "firing";
};

/** Rates at or above this print as whole numbers. */
const RATE_WHOLE = 10;
/** Significant digits of a smaller rate. */
const RATE_DIGITS = 2;
/** The smallest rate `round` keeps; a side with hits below it prints `<0.01`. */
const RATE_FLOOR = 0.01;

/**
 * A per-1k rate, spelled the same on every surface whatever its `count`
 * rounds to: `41`, `4.2`, `0.31`. A side with hits never reads `0`.
 */
export const rateText = (rate: number, hits: number): string => {
  if (hits > 0 && rate < RATE_FLOOR) {
    return `<${RATE_FLOOR}`;
  }
  return rate >= RATE_WHOLE
    ? Math.round(rate).toLocaleString("en-US")
    : String(Number(rate.toPrecision(RATE_DIGITS)));
};

/**
 * The rule check in one line, the same on every surface: where the rule sits,
 * then `quiet`, `not measured yet`, or the per-1k rate of each side that made
 * calls since the rule appeared.
 */
export const ruleLine = (rule: RuleCheck): string => {
  const day = rule.sinceIso?.slice(0, 10) ?? null;
  const since = day === null ? "" : ` since ${day}`;
  const place = `rule in ${rulePlace(rule)}`;
  switch (ruleState(rule)) {
    case "unmeasured":
      return `${place} · not measured yet${day === null ? "" : ` (added ${day})`}`;
    case "quiet":
      return `${place} · quiet${since}`;
    default:
      break;
  }
  const sides = [
    ["main", rule.callsSince.main, rule.hitsSince.main, rule.per1kSince.main],
    [
      "sub",
      rule.callsSince.nested,
      rule.hitsSince.nested,
      rule.per1kSince.nested,
    ],
  ] as const;
  const rates = sides
    .flatMap(([side, calls, hits, rate]) =>
      calls > 0 ? [`${side} ${rateText(rate, hits)}/1k`] : []
    )
    .join(", ");
  return `${place} · still firing ${rates}${since}`;
};

/**
 * The note a cause finding carries: the fix when its rule is quiet or absent,
 * else the rule line and its advice. A rule too new to measure says what to do
 * if the cause keeps firing, not the fix the file already states.
 */
export const causeNote = (row: CauseRow): string | null => {
  if (row.fix === null) {
    return null;
  }
  return row.rule === null || ruleState(row.rule) === "quiet"
    ? row.fix
    : `${ruleLine(row.rule)}: ${row.rule.advice}`;
};

/**
 * The memory line a cause suggests, or null. Only a `rule` cause that fired
 * and that no memory file already states gets one: a still-firing rule needs
 * an env or prompt fix instead, and an `env` cause needs the host changed.
 */
export const suggestedRuleOf = (row: CauseRow): string | null =>
  row.kind === "rule" && row.fix !== null && row.rule === null && row.hits > 0
    ? row.fix
    : null;

/** Every suggested memory line, in the cause table's order. */
export const suggestedRules = (report: StatsReport): readonly string[] =>
  report.causes.flatMap((row) => suggestedRuleOf(row) ?? []);

/**
 * Why there is no suggested rule, or null when there is one: no rule cause
 * fired, or every one that fired is already stated in a memory file.
 */
export const noSuggestionReason = (report: StatsReport): string | null => {
  if (suggestedRules(report).length > 0) {
    return null;
  }
  const fired = report.causes.some(
    (row) => row.kind === "rule" && row.fix !== null && row.hits > 0
  );
  return fired
    ? "every rule cause that fired is already stated in a memory file"
    : "no rule cause fired in this window";
};

const causeFinding = (row: CauseRow, f: FindingFormat): Finding | null => {
  const note = causeNote(row);
  return note === null || row.hits === 0
    ? null
    : {
        detector: row.cause,
        impact: causeImpact(row, f),
        key: row.key,
        note,
        rule: suggestedRuleOf(row),
        samples: row.samples,
        spread: causeSpread(row, f),
        table: "cause",
        title: row.label,
      };
};

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
        rule: null,
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
        rule: null,
        samples: row.samples,
        spread: timeSpread(row, bashSec, f),
        table: "time",
        title: row.label,
      };
};

/**
 * The cold-start list: rows that name a change, cheapest change first.
 *
 * Tier 0 is a named cause with a fix, in the cause table's order (retry
 * sessions, then sessions, then pairs). Tier 1 is a fail row whose `detector`
 * names the rule to change. Tier 2 is a fail row with a measured note and no
 * named rule. Tier 3 is a time row, which can only say how its seconds were
 * measured.
 */
export const findingsOf = (
  report: StatsReport,
  f: FindingFormat
): readonly Finding[] => {
  const fails = report.fails.flatMap((row) => failFinding(row, f) ?? []);
  return [
    ...report.causes.flatMap((row) => causeFinding(row, f) ?? []),
    ...fails.filter((one) => one.detector !== null),
    ...fails.filter((one) => one.detector === null),
    ...report.time.flatMap(
      (row) => timeFinding(row, report.corpus.bashSec, f) ?? []
    ),
  ];
};
