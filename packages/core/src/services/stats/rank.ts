/**
 * The determinism substrate: every comparator, percentile and record builder the
 * stats service is allowed to use.
 *
 * Two runs over an unchanged corpus must print byte-identical JSON, so no
 * detector may invent its own ordering. Each ranked surface has a named
 * comparator here: metric descending, then an explicit tiebreak, ending in a
 * string key that is unique. `Map`/`Set` iteration order never reaches output —
 * it is either sorted (`uniqSorted`, `sortedRecord`) or ranked. Floats leave
 * through `round`, which also normalises `-0`.
 */

/** Rows kept in a ranked table before the "N not shown" note. */
export const DEFAULT_ROW_LIMIT = 20;

/** Sample lines carried by one aggregate row. */
export const DEFAULT_SAMPLE_CAP = 3;

/**
 * Hits returned by one `drill` page. A drill is a whole-corpus pass, so a page
 * is sized to be scrolled through without asking for another one.
 */
export const DEFAULT_DRILL_PAGE = 200;

/** Session refs carried by one aggregate row; the true count is reported apart. */
export const SESSION_REF_CAP = 50;

/** Chars a sample line is clipped to after redaction. */
export const SAMPLE_LINE_MAX = 160;

/** Decimal places every emitted float is rounded to. */
export const FLOAT_DIGITS = 2;

/** A total ordering over `T`. Negative sorts `a` first. */
export type Compare<T> = (a: T, b: T) => number;

/** Plain ascending string compare, locale-independent. */
export const cmpStr = (a: string, b: string): number => {
  if (a < b) {
    return -1;
  }
  return a > b ? 1 : 0;
};

/** Compare by a numeric field, largest first. */
export const desc =
  <T>(metric: (x: T) => number): Compare<T> =>
  (a, b) =>
    metric(b) - metric(a);

/** Compare by a numeric field, smallest first. */
export const asc =
  <T>(metric: (x: T) => number): Compare<T> =>
  (a, b) =>
    metric(a) - metric(b);

/** Compare by a string field, ascending. */
export const ascStr =
  <T>(key: (x: T) => string): Compare<T> =>
  (a, b) =>
    cmpStr(key(a), key(b));

/** Apply comparators in order; the first non-zero wins. */
export const chain =
  <T>(...steps: readonly Compare<T>[]): Compare<T> =>
  (a, b) => {
    for (const step of steps) {
      const d = step(a, b);
      if (d !== 0) {
        return d;
      }
    }
    return 0;
  };

/** Sort a copy. The input is never mutated, so a fold order cannot leak. */
export const sortBy = <T>(rows: readonly T[], cmp: Compare<T>): T[] =>
  [...rows].sort(cmp);

/** Fail table: sessions spanned, then rows, then the key. Never cluster count. */
export const compareFailRows = chain<{
  sessions: number;
  rows: number;
  key: string;
}>(
  desc((r) => r.sessions),
  desc((r) => r.rows),
  ascStr((r) => r.key)
);

/** Time table: seconds, then runs, then the job key. */
export const compareTimeRows = chain<{
  totalSec: number;
  runs: number;
  key: string;
}>(
  desc((r) => r.totalSec),
  desc((r) => r.runs),
  ascStr((r) => r.key)
);

/** Error clusters: rows, then sessions spanned, then the signature. */
export const compareClusterRows = chain<{
  rows: number;
  sessions: number;
  key: string;
}>(
  desc((r) => r.rows),
  desc((r) => r.sessions),
  ascStr((r) => r.key)
);

/** Command families: count, then the family string. */
export const compareFamilyRows = chain<{ count: number; family: string }>(
  desc((r) => r.count),
  ascStr((r) => r.family)
);

/** Session refs inside a row: hits, then the session id. */
export const compareSessionRefs = chain<{ hits: number; sid: string }>(
  desc((r) => r.hits),
  ascStr((r) => r.sid)
);

/** Any keyed row with a single numeric metric: metric, then key. */
export const compareKeyed = <T extends { key: string }>(
  metric: (x: T) => number
): Compare<T> =>
  chain<T>(
    desc(metric),
    ascStr((r) => r.key)
  );

/**
 * Facts inside one session: call time, then `seq`. Never `ts` alone — rows are
 * emitted when the result parses while `ts` is the call time, so 1.3% of
 * adjacent within-session pairs go backwards and 5.9% share a second.
 */
export const compareFacts = chain<{ ts: string | null; seq: number }>(
  ascStr((f) => f.ts ?? ""),
  asc((f) => f.seq)
);

/** Transcripts: absolute path. Directory read order is never the walk order. */
export const compareByPath = ascStr<{ path: string }>((r) => r.path);

/** Sum a numeric field. 0 for an empty list. */
export const sum = <T>(items: readonly T[], f: (x: T) => number): number =>
  items.reduce((acc, x) => acc + f(x), 0);

/** Round to `digits` places and normalise `-0` to `0`. */
export const round = (n: number, digits = FLOAT_DIGITS): number => {
  if (!Number.isFinite(n)) {
    return 0;
  }
  const p = 10 ** digits;
  const v = Math.round(n * p) / p;
  return Object.is(v, -0) ? 0 : v;
};

/** Divide, returning 0 when the denominator is 0 or not finite. */
export const safeDiv = (a: number, b: number): number =>
  b === 0 || !Number.isFinite(b) ? 0 : a / b;

/** A share of a total, rounded to four places so it survives a JSON diff. */
const SHARE_DIGITS = 4;
export const share = (part: number, total: number): number =>
  round(safeDiv(part, total), SHARE_DIGITS);

/**
 * Nearest-rank percentile. `p` is a fraction in [0,1]; n=1 returns that value;
 * an empty list returns 0. One interpolation rule for the whole service.
 */
export const percentile = (values: readonly number[], p: number): number => {
  if (values.length === 0) {
    return 0;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(p * sorted.length) - 1)
  );
  return sorted[idx] ?? 0;
};

/** Median of an unsorted list. 0 for an empty list. */
export const median = (values: readonly number[]): number => {
  if (values.length === 0) {
    return 0;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  if (sorted.length % 2 === 1) {
    return sorted[mid] ?? 0;
  }
  return ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2;
};

/** Increment a counter in a `Map`. */
export const bump = (
  counts: Map<string, number>,
  key: string,
  by = 1
): void => {
  counts.set(key, (counts.get(key) ?? 0) + by);
};

/** Sorted unique strings. */
export const uniqSorted = (values: Iterable<string>): string[] =>
  [...new Set(values)].sort(cmpStr);

/** Key with the highest count; ties break on the smallest key. */
export const topKey = (counts: Map<string, number>): string => {
  let best = "";
  let bestN = -1;
  for (const key of uniqSorted(counts.keys())) {
    const n = counts.get(key) ?? 0;
    if (n > bestN) {
      best = key;
      bestN = n;
    }
  }
  return best;
};

/** Build a `Record` with keys inserted in sorted order. Every map that leaves
 * core goes through this, so insertion order can never leak into JSON. */
export const sortedRecord = <T>(
  entries: Iterable<readonly [string, T]>
): Record<string, T> => {
  const out: Record<string, T> = {};
  for (const [key, value] of [...entries].sort((a, b) => cmpStr(a[0], b[0]))) {
    out[key] = value;
  }
  return out;
};

/** A capped ranking that still reports how many rows it dropped. */
export interface Ranking<T> {
  readonly dropped: number;
  readonly rows: readonly T[];
  readonly total: number;
}

/** Sort by `cmp`, keep the first `limit` rows, and report the remainder. */
export const rank = <T>(
  rows: readonly T[],
  cmp: Compare<T>,
  limit = DEFAULT_ROW_LIMIT
): Ranking<T> => {
  const sorted = sortBy(rows, cmp);
  const cap = Math.max(0, Math.floor(limit));
  const kept = sorted.slice(0, cap);
  return {
    rows: kept,
    total: sorted.length,
    dropped: sorted.length - kept.length,
  };
};

/** Collapse whitespace runs to single spaces and trim. */
export const collapseWs = (s: string): string => s.replace(/\s+/g, " ").trim();

/** Cap a string, marking the cut with one ellipsis char. */
export const clip = (s: string, max: number): string =>
  s.length > max ? `${s.slice(0, max)}…` : s;

/**
 * Choose sample lines: shortest first, then string compare, then session id.
 * Never "first seen" — that is transcript order, which is not stable across a
 * corpus whose files change.
 */
export const pickSamples = <
  T extends { line: string; sid: string; seq: number },
>(
  candidates: readonly T[],
  cap = DEFAULT_SAMPLE_CAP
): readonly T[] => {
  const seen = new Set<string>();
  const unique: T[] = [];
  for (const c of candidates) {
    const id = `${c.sid}#${c.seq}`;
    if (!seen.has(id)) {
      seen.add(id);
      unique.push(c);
    }
  }
  return sortBy(
    unique,
    chain<T>(
      asc((c) => c.line.length),
      ascStr((c) => c.line),
      ascStr((c) => c.sid)
    )
  ).slice(0, Math.max(0, cap));
};
