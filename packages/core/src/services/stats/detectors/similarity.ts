/**
 * How alike a failed call and the call after it are.
 *
 * The 0.6 threshold and its measured precision (issue #56: a hand-labelled
 * sample of 40 fail → next-call pairs from real transcripts, kept outside the
 * repo because it holds real commands; see `retries-sample.test.ts`) were
 * scored with Python's `difflib.SequenceMatcher(None, a[:800], b[:800])
 * .ratio()`, so the ratio here is an exact port of Ratcliff/Obershelp as
 * difflib runs it, `autojunk` included: once `b` has 200 or more chars, every char occurring in more than
 * 1% + 1 of its positions is "popular" and cannot seed a match (it can still
 * extend one). A plain LCS ratio scores differently and would move every pair
 * near the threshold. Strings are compared by code point, as Python does.
 *
 * The hybrid score compares only the failing segment when the error names the
 * culprit (a glob word, a missing binary, a blocked sleep), because a long chain
 * whose one segment was fixed reads as a different command on the whole line.
 */

/** Chars of each side difflib compared in the #56 measurement. */
const RATIO_MAX = 800;

/** `b` length at which difflib's autojunk heuristic switches on. */
const AUTOJUNK_MIN = 200;

/** Percent of `b` a char must exceed to be popular: `n // 100 + 1`. */
const AUTOJUNK_DIVISOR = 100;

/** Score a sleep-blocked pair gets when the retry runs the blocked command. */
const SLEEP_HIT = 0.9;

/** One matching block: `a[i:i+k] == b[j:j+k]`. */
interface Block {
  readonly i: number;
  readonly j: number;
  readonly k: number;
}

/** A string as Python indexes it: code points, cut to `max`. */
type Points = readonly number[];

const SURROGATE_MIN = 0xd8_00;
const ASCII = 128;
/** Largest code point a single UTF-16 unit holds. */
const BMP_MAX = 0xff_ff;
const SURROGATE_MAX = 0xdf_ff;

/** Code points of `s`, at most `max`. Plain UTF-16 is read without a copy. */
const pointsOf = (s: string, max: number): Points => {
  const out: number[] = [];
  for (let i = 0; i < s.length && out.length < max; i++) {
    const c = s.charCodeAt(i);
    if (c >= SURROGATE_MIN && c <= SURROGATE_MAX) {
      const cp = s.codePointAt(i) ?? c;
      out.push(cp);
      if (cp > BMP_MAX) {
        i += 1;
      }
    } else {
      out.push(c);
    }
  }
  return out;
};

/** Index of every non-popular point of `b`, ascending, as difflib's `b2j`. */
const indexB = (b: Points): Map<number, number[]> => {
  const b2j = new Map<number, number[]>();
  for (let j = 0; j < b.length; j++) {
    const ch = b[j] ?? 0;
    const list = b2j.get(ch);
    if (list) {
      list.push(j);
    } else {
      b2j.set(ch, [j]);
    }
  }
  if (b.length >= AUTOJUNK_MIN) {
    const ntest = Math.floor(b.length / AUTOJUNK_DIVISOR) + 1;
    for (const [ch, list] of [...b2j.entries()]) {
      if (list.length > ntest) {
        b2j.delete(ch);
      }
    }
  }
  return b2j;
};

/** Scratch rows for `longestMatch`: `j2len` keyed by j + 1, reset by hand. */
interface Scratch {
  cur: Int32Array;
  curTouched: number[];
  next: Int32Array;
  nextTouched: number[];
}

/** Zero the entries a row set, so the array is clean for its next use. */
const clearRow = (row: Int32Array, touched: number[]): void => {
  for (const t of touched) {
    row[t] = 0;
  }
  touched.length = 0;
};

/** difflib's `find_longest_match`, with no junk predicate. */
const longestMatch = (args: {
  readonly a: Points;
  readonly b: Points;
  readonly b2j: ReadonlyMap<number, readonly number[]>;
  readonly scratch: Scratch;
  readonly alo: number;
  readonly ahi: number;
  readonly blo: number;
  readonly bhi: number;
}): Block => {
  const { a, b, b2j, scratch, alo, ahi, blo, bhi } = args;
  let besti = alo;
  let bestj = blo;
  let bestsize = 0;
  for (let i = alo; i < ahi; i++) {
    const { cur, next, nextTouched } = scratch;
    for (const j of b2j.get(a[i] ?? -1) ?? []) {
      if (j < blo) {
        continue;
      }
      if (j >= bhi) {
        break;
      }
      const k = (cur[j] ?? 0) + 1;
      next[j + 1] = k;
      nextTouched.push(j + 1);
      if (k > bestsize) {
        besti = i - k + 1;
        bestj = j - k + 1;
        bestsize = k;
      }
    }
    clearRow(scratch.cur, scratch.curTouched);
    scratch.cur = next;
    scratch.next = cur;
    const touched = scratch.curTouched;
    scratch.curTouched = nextTouched;
    scratch.nextTouched = touched;
  }
  clearRow(scratch.cur, scratch.curTouched);
  // Popular points never seed a match but may extend one; with no junk
  // predicate, difflib extends over any equal point on both sides.
  while (besti > alo && bestj > blo && a[besti - 1] === b[bestj - 1]) {
    besti -= 1;
    bestj -= 1;
    bestsize += 1;
  }
  while (
    besti + bestsize < ahi &&
    bestj + bestsize < bhi &&
    a[besti + bestsize] === b[bestj + bestsize]
  ) {
    bestsize += 1;
  }
  return { i: besti, j: bestj, k: bestsize };
};

/** Total matched points over difflib's matching blocks. */
const matchedPoints = (a: Points, b: Points): number => {
  const b2j = indexB(b);
  const scratch: Scratch = {
    cur: new Int32Array(b.length + 1),
    curTouched: [],
    next: new Int32Array(b.length + 1),
    nextTouched: [],
  };
  const queue: [number, number, number, number][] = [
    [0, a.length, 0, b.length],
  ];
  let total = 0;
  while (queue.length > 0) {
    const [alo, ahi, blo, bhi] = queue.pop() ?? [0, 0, 0, 0];
    const { i, j, k } = longestMatch({
      a,
      b,
      b2j,
      scratch,
      alo,
      ahi,
      blo,
      bhi,
    });
    if (k === 0) {
      continue;
    }
    total += k;
    if (alo < i && blo < j) {
      queue.push([alo, i, blo, j]);
    }
    if (i + k < ahi && j + k < bhi) {
      queue.push([i + k, ahi, j + k, bhi]);
    }
  }
  return total;
};

const ratioOfPoints = (a: Points, b: Points): number => {
  const length = a.length + b.length;
  return length === 0 ? 1 : (2 * matchedPoints(a, b)) / length;
};

/**
 * difflib's `quick_ratio`: the multiset overlap of the two strings. An upper
 * bound on `ratio`, so a pair under the floor here is under it there too.
 */
const quickRatioOfPoints = (a: Points, b: Points): number => {
  const length = a.length + b.length;
  if (length === 0) {
    return 1;
  }
  const ascii = new Int32Array(ASCII);
  const wide = new Map<number, number>();
  for (const ch of b) {
    if (ch < ASCII) {
      ascii[ch] = (ascii[ch] ?? 0) + 1;
    } else {
      wide.set(ch, (wide.get(ch) ?? 0) + 1);
    }
  }
  let matches = 0;
  for (const ch of a) {
    if (ch < ASCII) {
      if ((ascii[ch] ?? 0) > 0) {
        ascii[ch] = (ascii[ch] ?? 0) - 1;
        matches += 1;
      }
      continue;
    }
    const left = wide.get(ch) ?? 0;
    if (left > 0) {
      wide.set(ch, left - 1);
      matches += 1;
    }
  }
  return (2 * matches) / length;
};

/**
 * `difflib.SequenceMatcher(None, a, b).ratio()`, exactly. Two empty strings
 * score 1, as in Python.
 */
export const ratio = (a: string, b: string, max = RATIO_MAX): number =>
  ratioOfPoints(pointsOf(a, max), pointsOf(b, max));

/**
 * `ratio`, or 0 when its upper bounds already put it under `floor`. Scores at
 * or above the floor are exact.
 */
const ratioAtLeast = (a: string, b: string, floor: number): number => {
  const pa = pointsOf(a, RATIO_MAX);
  const pb = pointsOf(b, RATIO_MAX);
  const length = pa.length + pb.length;
  if (length > 0 && (2 * Math.min(pa.length, pb.length)) / length < floor) {
    return 0;
  }
  if (quickRatioOfPoints(pa, pb) < floor) {
    return 0;
  }
  return ratioOfPoints(pa, pb);
};

const QUOTES = /["']/g;
const ESCAPED_PIPE = /\\\|/g;
const WS_RUN = /\s+/g;

/**
 * One segment as the #56 measurement compared it: quotes stripped, a grep BRE `\|`
 * folded into `|`, whitespace collapsed. `*.ts` and `'*.ts'` are then equal.
 */
export const normSeg = (s: string): string =>
  s.replace(QUOTES, "").replace(ESCAPED_PIPE, "|").replace(WS_RUN, " ").trim();

/**
 * Chain separators, as the #56 measurement split on them: not quote-aware, on
 * purpose, so the shipped score stays the one whose precision was measured.
 */
const SEGMENT_SPLIT = /\s*(?:&&|\|\||;|\||\n)\s*/;

/** Segments that carry no intent of their own: separators, cd, output trims. */
const NOISE_PREFIXES = [
  "echo",
  "cd ",
  "head",
  "tail -",
  "wc",
  "sort",
  "uniq",
] as const;

/**
 * The normalised working segments of a command line. A line made only of noise
 * segments is kept whole, so it is never empty.
 */
export const segments = (cmd: string): readonly string[] => {
  const kept = cmd
    .split(SEGMENT_SPLIT)
    .filter((x) => {
      const t = x.trim();
      return t !== "" && !NOISE_PREFIXES.some((p) => t.startsWith(p));
    })
    .map(normSeg);
  return kept.length > 0 ? kept : [normSeg(cmd)];
};

/** Separator between the segments stored in `ToolFact.focus`. */
export const FOCUS_SEP = " ␞ ";

/** How a failed call's focus was cut, which picks the comparison mode. */
export type FocusMode = "glob" | "cnf" | "sleep" | "whole";

/** Everything the hybrid score reads from one call. */
export interface SimilarityInput {
  /** Failing segments, `FOCUS_SEP`-joined; null scores the whole line. */
  readonly focus: string | null;
  readonly mode: FocusMode;
  readonly shell: boolean;
  /** Command line for a shell call; the primary argument otherwise. */
  readonly text: string;
}

/** The score and the retry segment that earned it. */
export interface SimilarityHit {
  readonly score: number;
  /** The retry segment that scored best (before any word was dropped); the
   * whole line in whole mode. */
  readonly segment: string;
}

const dropFirstWord = (s: string): string => s.split(" ").slice(1).join(" ");

const lastWord = (s: string): string => s.split(" ").at(-1) ?? "";

/**
 * The hybrid similarity of #56: when the failed call names its culprit,
 * compare its failing segment(s) against every retry segment and keep the best;
 * otherwise compare the whole line.
 */
export const hybridSimilarity = (
  failed: SimilarityInput,
  retry: string,
  /** Scores under this may be reported as 0: a caller that only keeps pairs
   * at or above it skips the exact ratio when an upper bound rules it out. */
  floor = 0,
  /** Splits the retry; a caller scoring one retry against several seeds
   * passes a memoised one. */
  split: (cmd: string) => readonly string[] = segments
): SimilarityHit => {
  const score = (a: string, b: string, bar: number) =>
    bar > 0 ? ratioAtLeast(a, b, bar) : ratio(a, b);
  if (!(failed.shell && failed.focus !== null && failed.mode !== "whole")) {
    return { score: score(failed.text, retry, floor), segment: retry };
  }
  const focus = failed.focus.split(FOCUS_SEP).filter((s) => s !== "");
  const base = split(retry).map((seg) => ({ cmp: seg, seg }));
  // A missing binary is retried with another one: also compare its arguments.
  const candidates =
    failed.mode === "cnf"
      ? [
          ...base,
          ...base.map((c) => ({ cmp: dropFirstWord(c.seg), seg: c.seg })),
        ]
      : base;
  let best: SimilarityHit = { score: 0, segment: base[0]?.seg ?? "" };
  for (const f of focus) {
    for (const { cmp, seg } of candidates) {
      // Only a higher score replaces the best, so its bound must beat both.
      let s = score(f, cmp, floor > 0 ? Math.max(floor, best.score) : 0);
      if (failed.mode === "sleep" && cmp.includes(lastWord(f))) {
        s = Math.max(s, SLEEP_HIT);
      }
      if (s > best.score) {
        best = { score: s, segment: seg };
      }
    }
  }
  return best;
};
