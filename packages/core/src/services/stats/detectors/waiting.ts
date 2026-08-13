/**
 * Waiting: the shell is parked on the clock, or on a condition it re-checks,
 * instead of running a command that does work.
 *
 * One function serves both surfaces. `waitShape` decides the shape, `isWaiting`
 * is `waitShape(cmd) !== null`, and the job classifier calls `isWaiting` at rank
 * 1 — so the `wait/poll` row of the time table and this panel are the same set
 * of rows by construction, not by agreement.
 *
 * Text is prepared in three steps before any pattern applies: drop heredoc
 * bodies (authoring a file is not waiting), drop `#` comment lines (a prose
 * apostrophe desynchronises the quote mask and swallows the rest of the line),
 * then blank quoted spans. Structure matches on the masked text, delay tokens on
 * the unmasked text: `grep -iE "vite|watch"` is not a `watch` call, while a real
 * delay is usually quoted.
 *
 * A delay is a delay however it is spelled: `sleep N`, `/bin/sleep N`, `sleep .5`,
 * `usleep N`, `read -t N`, `perl -e 'select undef,undef,undef,N'`,
 * `python -c time.sleep(N)`. Inside an interpreter payload the delay must be 10s
 * or more, which keeps `time.sleep(560)` and drops the 1.2s pause between
 * rate-limited API calls. `setTimeout(resolve, ms)` is not a delay: it is how a
 * browser-automation payload defines a sleep helper.
 *
 * Excluded on purpose: the `wait` builtin (a parallel fan-out, where the seconds
 * are the fetches), a loop holding a probe but no delay, `while read` and
 * `while IFS=` list iteration, and a `--watch` flag, since `watch` must be the
 * command.
 *
 * Backgrounded waits stay in the set and get their own column, never their own
 * total: `declaredDelaySec` prints beside the charged seconds and is never
 * summed into one.
 */
import {
  type AccumulatorFactory,
  addToTally,
  hitOf,
  sampleOf,
  samplesOf,
  type Tally,
  tallyFor,
} from "../accumulate";
import {
  asc,
  ascStr,
  type Compare,
  chain,
  compareKeyed,
  desc,
  median,
  percentile,
  round,
  share,
  sortBy,
} from "../rank";
import {
  type DrillHit,
  isBashFact,
  type WaitPanel,
  type WaitShape,
  type WaitShapeRow,
} from "../schema";

/** The job key a waiting line is charged to. Rank 1 of the priority order. */
const WAIT_JOB = "wait/poll";

/** Drill key that asks for every waiting row rather than one shape. */
const ALL = "all";

/** Percentile reported beside the median. */
const P95 = 0.95;

/** Every shape, so a zero row still prints and the panel keeps its columns. */
const WAIT_SHAPES = [
  "loop-poll",
  "watcher",
  "curl-retry",
  "bare-delay",
] as const satisfies readonly WaitShape[];

/**
 * Drill hits, longest first: a drill answers "which calls cost this", and the
 * service keeps only the head of the list.
 */
export const compareDrillHits: Compare<DrillHit> = chain<DrillHit>(
  desc((h) => h.durSec ?? 0),
  ascStr((h) => h.ts ?? ""),
  ascStr((h) => h.sid),
  asc((h) => h.seq)
);

const HEREDOC_OPEN = /<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)['"]?/;
const COMMENT_LINE = /^\s*#/;
const SINGLE_QUOTED = /'[^']*'/g;
const DOUBLE_QUOTED = /"[^"]*"/g;

/** Drop every heredoc body, keeping the line that opens it. */
export const stripHeredocBodies = (cmd: string) => {
  const out: string[] = [];
  let tag: string | null = null;
  for (const line of cmd.split("\n")) {
    if (tag !== null) {
      if (line.trim() === tag) {
        tag = null;
      }
      continue;
    }
    out.push(line);
    const open = HEREDOC_OPEN.exec(line);
    if (open) {
      tag = open[1] ?? null;
    }
  }
  return out.join("\n");
};

/** Drop lines whose first non-blank character is `#`. */
export const stripComments = (cmd: string) =>
  cmd
    .split("\n")
    .filter((line) => !COMMENT_LINE.test(line))
    .join("\n");

/** Blank the contents of every quoted span, keeping the quotes. */
export const maskQuoted = (cmd: string) =>
  cmd.replace(SINGLE_QUOTED, "''").replace(DOUBLE_QUOTED, '""');

/** Start of line, or right after `;` `&&` `||` `|` `&` `(` a newline, `do`, `then`.
 * The bare `&` is required, or `srv & sleep 25` is lost. */
const AT_CMD = String.raw`(?:^|[;&|(\n]|\bdo\b|\bthen\b)\s*`;

const LOOP_HEAD = new RegExp(`${AT_CMD}(?:while|until)\\s*(?=[\\s[!])`);
const FOR_HEAD = new RegExp(`${AT_CMD}for\\s+\\w+\\s+in\\b`);
const LIST_ITERATION = /\b(?:while|until)\s+(?:read\b|IFS=)/;
/** `/bin/sleep 30` and `sleep .5` count: the path prefix hides a real delay. */
const DELAY_AT_CMD = new RegExp(
  `${AT_CMD}(?:\\S*/)?(?:sleep|usleep)\\s+\\.?\\d`
);
const DELAY_ANYWHERE =
  /(?:\bsleep\s+\.?\d|\busleep\s+\d|\bread\s+-t\s*\d|select\s+undef\s*,\s*undef\s*,\s*undef|\btime\.sleep\s*\(\s*\.?\d)/;
/** A delay of 10s or more inside an interpreter payload: waiting, not rate limiting. */
const LONG_INLINE_DELAY =
  /(?:\btime\.sleep\s*\(\s*\d{2,}|select\s+undef\s*,\s*undef\s*,\s*undef\s*,\s*\d{2,})/;
const INTERPRETER = /\b(?:python3?|perl|node|bun|ruby)\s+-[ce]\b/;
const LOOP_ANY = /\b(?:while|until|for)\b/;
const WATCHER = new RegExp(
  `${AT_CMD}(?:watch|wait-on|wait-for-it(?:\\.sh)?|dockerize)\\b|\\bgh\\s+run\\s+watch\\b|\\btail\\s+(?:-[a-zA-Z]*f|--follow)\\b`
);
const CURL_RETRY = /\bcurl\b[^\n]*--retry/;

/** The waiting shape of a command, or null. First match wins. */
export const waitShape = (cmd: string): WaitShape | null => {
  const text = stripComments(stripHeredocBodies(cmd));
  const struct = maskQuoted(text);
  const shellLoop =
    (LOOP_HEAD.test(struct) || FOR_HEAD.test(struct)) &&
    !LIST_ITERATION.test(struct);
  const inlineLoop =
    INTERPRETER.test(struct) &&
    LOOP_ANY.test(text) &&
    LONG_INLINE_DELAY.test(text);
  if ((shellLoop && DELAY_ANYWHERE.test(text)) || inlineLoop) {
    return "loop-poll";
  }
  if (WATCHER.test(struct)) {
    return "watcher";
  }
  if (CURL_RETRY.test(struct)) {
    return "curl-retry";
  }
  if (DELAY_AT_CMD.test(struct)) {
    return "bare-delay";
  }
  if (INTERPRETER.test(struct) && LONG_INLINE_DELAY.test(text)) {
    return "bare-delay";
  }
  return null;
};

/** `waitShape(cmd) !== null`. The one call the job classifier makes at rank 1. */
export const isWaiting = (cmd: string) => waitShape(cmd) !== null;

const SLEEP_LITERAL = /\b(?:\S*\/)?sleep\s+(\.?\d[\d.]*)/g;
const PY_SLEEP_LITERAL = /\btime\.sleep\s*\(\s*(\.?\d[\d.]*)/g;
const PERL_SLEEP_LITERAL =
  /select\s+undef\s*,\s*undef\s*,\s*undef\s*,\s*(\.?\d[\d.]*)/g;

/** The literal seconds a line asked to sleep for: a lower bound on a
 * backgrounded wait, and never part of a charged total. */
export const declaredDelaySec = (cmd: string) => {
  const text = stripComments(stripHeredocBodies(cmd));
  let total = 0;
  for (const re of [SLEEP_LITERAL, PY_SLEEP_LITERAL, PERL_SLEEP_LITERAL]) {
    for (const hit of text.matchAll(re)) {
      total += Number(hit[1]) || 0;
    }
  }
  return total;
};

/**
 * The waiting panel. Aggregates bash facts whose `job` is `wait/poll`, breaks
 * them down by `wait`, and divides by `ctx.corpus.bashSec` through `share`.
 */
export const makeWaitPanel: AccumulatorFactory<WaitPanel> = (opts) => {
  // One tally under ALL, one per shape. A shape name is never `all`.
  const tallies = new Map<string, Tally>();
  const hits: DrillHit[] = [];
  const wanted = opts.drill?.table === "wait" ? opts.drill.key : null;
  let bgRuns = 0;
  let bgSec = 0;
  let bgDeclaredSec = 0;

  return {
    id: "waiting",

    add: (fact) => {
      if (!isBashFact(fact) || fact.job !== WAIT_JOB) {
        return;
      }
      const line = opts.redact.line(fact.cmd);
      const sample = sampleOf(fact, line);
      addToTally({ tally: tallyFor(tallies, ALL), fact, sample });
      if (fact.wait !== null) {
        addToTally({ tally: tallyFor(tallies, fact.wait), fact, sample });
      }
      if (fact.bg) {
        bgRuns += 1;
        bgSec += fact.durSec ?? 0;
        bgDeclaredSec += fact.declaredDelaySec;
      }
      const matches =
        wanted !== null &&
        (wanted === ALL || wanted === WAIT_JOB || wanted === fact.wait);
      if (matches) {
        hits.push(hitOf(fact, line));
      }
    },

    finish: (ctx) => {
      const total = tallyFor(tallies, ALL);
      const rows = WAIT_SHAPES.map((key) => {
        const tally = tallyFor(tallies, key);
        return {
          key,
          calls: tally.rows,
          totalSec: round(tally.totalSec),
          share: share(tally.totalSec, ctx.corpus.bashSec),
        } satisfies WaitShapeRow;
      });
      return {
        calls: total.rows,
        totalSec: round(total.totalSec),
        share: share(total.totalSec, ctx.corpus.bashSec),
        sessions: total.sessions.size,
        projects: total.projects.size,
        medianSec: round(median(total.durations)),
        p95Sec: round(percentile(total.durations, P95)),
        byShape: sortBy(
          rows,
          compareKeyed<WaitShapeRow>((r) => r.totalSec)
        ),
        bgRuns,
        bgSec: round(bgSec),
        bgDeclaredSec: round(bgDeclaredSec),
        samples: samplesOf(total, ctx.sampleCap),
      };
    },

    drill: () => sortBy(hits, compareDrillHits),
  };
};
