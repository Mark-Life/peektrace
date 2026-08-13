/**
 * What failed, on two inputs: the agent's own error flag, and an output scanner
 * run at scan time.
 *
 * The flag alone is the wrong signal — 1,070 bash calls printed a failure and
 * exited 0, against 961 that set the flag at all. The scanner runs while the
 * result text is still in hand and stores `failed` + `failReason`, never the
 * text, so a later cap on stored output cannot lose a failure.
 *
 * Order matters. `zsh-equals-abort` is a detector, not a filter: an unquoted
 * `echo ===` separator EQUALS-expands, exits 1, and the rest of the line never
 * runs. It marks the row and must run before any suppression, because the four
 * benign filters would otherwise swallow it. The filters then run in order:
 * `glob-no-match` (zsh "no matches found"), `absence-probe` (a failing existence
 * check reporting absence), `chained-tail` (output printed before a failing
 * tail), `user-rejected` (the call never ran). Together they suppress 324 of 961
 * errored bash rows at 78% precision and 64% recall on the labelled sample;
 * `chained-tail` unfiltered is 42% precise and must keep the detector's rows
 * out. Non-bash errors are never filtered.
 *
 * Rows rank by sessions spanned, then rows, then key — never by cluster count.
 */
import {
  type AccumulatorFactory,
  addToTally,
  type FinishContext,
  hitOf,
  sampleOf,
  samplesOf,
  type Tally,
  tallyFor,
} from "../accumulate";
import {
  asc,
  ascStr,
  bump,
  chain,
  compareFailRows,
  rank,
  round,
  share,
  sortBy,
  topKey,
} from "../rank";
import {
  type CallFact,
  type DrillHit,
  type FailRow,
  isBashFact,
  isCallFact,
  type StatsFact,
} from "../schema";

/** Id of the one named detector this corpus supports. */
export const ZSH_EQUALS_ABORT = "zsh-equals-abort";

/** Id carried by a silent-failure row whose exit code a pipe discarded. */
const PIPED_EXIT_CODE = "piped-exit-code";

/**
 * One entry per failure shape. Each pattern keys on a token a tool only prints
 * when something is wrong, never on an English word the agent could have echoed.
 * `lint` and `test` are the two that needed care: a bare `/lint\//` matched a
 * workflow description, and an unconstrained digit in `/\d+ fail/` matched `bun
 * test` printing `0 fail` on a **passing** run, which alone added 626 false rows.
 */
const SIGNATURES = [
  // tsc / tsgo, e.g. "src/x.ts(12,3): error TS2339: ..."
  { id: "tsc", label: "error TS####", re: /\berror TS\d{4}\b/ },
  // biome / eslint diagnostic carrying a file:line:col, plus biome's summary
  {
    id: "lint",
    label: "lint diagnostic",
    re: /(?:[\w./-]+:\d+:\d+\s+lint\/[a-z]+\/\w+|\bFound \d+ errors?\b)/,
  },
  // vitest / jest / bun test summaries. Every count must be non-zero.
  {
    id: "test",
    label: "N failing tests",
    re: /(?:Tests\s+[1-9]\d* failed|\([1-9]?\d* ?tests? \| [1-9]\d* failed\)|\bFAIL\s+[\w./-]*\.(?:test|spec)\.[jt]sx?\b|\b[1-9]\d* fail\b)/,
  },
  {
    id: "traceback",
    label: "python traceback",
    re: /Traceback \(most recent call last\):/,
  },
  // a real V8 frame: "    at fn (/path/file.ts:12:3)" or "    at /path/file.ts:12"
  {
    id: "node",
    label: "node stack frame",
    re: /\n\s+at [^\n]*(?::\d+:\d+\)|\.(?:ts|tsx|js|mjs|cjs):\d+)/,
  },
] as const;

/**
 * Tools that return shell output. The scanner reads a compiler summary as a
 * failure, so it must not read one out of a file the agent merely opened.
 */
const SHELL_TOOLS: ReadonlySet<string> = new Set([
  "Bash",
  "bash",
  "shell",
  "local_shell",
  "run_command",
  "execute_command",
]);

/**
 * The habit that discards the exit code: zsh has no `pipefail`, so the status of
 * `tsc | tail` is tail's. 1,004 of the 1,070 silent failures pipe this way.
 */
const PIPED =
  /\|\s*(?:head|tail|grep|egrep|rg|wc|jq|sed|awk|tee|sort|uniq|cut)\b/;

/**
 * A bare `echo ===` separator at a command position. zsh EQUALS-expands `===`,
 * fails to find the command, exits 1, and never runs the rest of the line. The
 * boundary is widened to any whitespace because the fact stores a
 * whitespace-collapsed command, which turns a newline separator into a space;
 * over the corpus both forms select the same 200 rows.
 */
const ZSH_EQUALS = /(^|[\s;&|])echo\s+=/;

/** Claude's shell results open with the exit status; a clean run has no header. */
const EXIT_HEADER = /^Exit code (\d+)/;

const EXIT_PREFIX = /^Exit code \d+\n?/;

/** Segment separators of a chain: the last one owns the exit code. */
const CHAIN = /;|&&|\|\||\n/;

/** Binaries whose failure is an answer: the thing asked about is not there. */
const ABSENCE_BINS: ReadonlySet<string> = new Set([
  "ls",
  "which",
  "type",
  "stat",
  "test",
  "find",
  "du",
  "wc",
  "file",
]);

const ABSENT_TEXT = /No such file or directory|not found|does not exist/i;

/** Text that says the failure was real, whatever the chain printed before it. */
const ERROR_TEXT =
  /Traceback|error TS\d|SyntaxError|TypeError|KeyError|ValueError|AssertionError|Cannot find module|ERR_MODULE_NOT_FOUND|fatal:|ModuleNotFoundError|timed out|rate limit|<tool_use_error>|FAIL |error:/;

const USER_REJECTED = /The user doesn't want to proceed with this tool use/;

const GLOB_NO_MATCH = /no matches found/;

const GUARDED_TAIL = /2>\s*\/dev\/null/;

/** Chars of output below which a chain has not yet printed its work. */
const CHAINED_TAIL_MIN_CHARS = 200;

const ENV_ASSIGNMENT = /^[A-Z_]+=/;

const LEADING_PATH = /^.*\//;

const WHITESPACE = /\s+/;

/** A share rendered as a percentage in a caveat line. */
const PERCENT = 100;

/** What the output scanner found in one tool result. */
export interface OutputScan {
  /** Detector id when a named rule fired, e.g. "zsh-equals-abort". */
  readonly detector: string | null;
  /** The agent's flag, or the scanner firing on an exit-0 result. */
  readonly failed: boolean;
  /** "tsc" | "lint" | "test" | "traceback" | "node" | ... , else null. */
  readonly reason: string | null;
}

/**
 * Scan one tool result for a failure the exit code did not report.
 *
 * `detector` stays null here: the only named detector this corpus supports keys
 * on the command, not on the output, and the shell's own EQUALS message reaches
 * the result in 196 of 200 rows while three unrelated rows quote it. The fail
 * table marks those rows from the command instead, which is exact.
 */
export const scanOutput = (args: {
  readonly tool: string;
  readonly text: string;
  readonly isError: boolean;
}): OutputScan => {
  const hit = SHELL_TOOLS.has(args.tool)
    ? SIGNATURES.find((s) => s.re.test(args.text))
    : undefined;
  return {
    detector: null,
    failed: args.isError || hit !== undefined,
    reason: hit?.id ?? null,
  };
};

/** True when a pipe discards the exit code of the command that failed. */
export const isPiped = (cmd: string): boolean => PIPED.test(cmd);

/** True when a shell actually ran and reported a non-zero status. */
const ranAndFailed = (output: string): boolean => EXIT_HEADER.test(output);

/** The output with the exit-status header removed. */
const bodyOf = (output: string): string =>
  output.replace(EXIT_PREFIX, "").trim();

/** Last segment of a chain: the command whose status the row carries. */
const tailOf = (cmd: string): string => {
  const parts = cmd.split(CHAIN).filter((s) => s.trim().length > 0);
  return (parts.at(-1) ?? cmd).trim();
};

/** The binary a segment runs, with its path and any env prefix stripped. */
const firstWord = (segment: string): string => {
  const words = segment
    .trim()
    .split(WHITESPACE)
    .filter((w) => !ENV_ASSIGNMENT.test(w));
  return (words[0] ?? "").replace(LEADING_PATH, "");
};

const isChain = (cmd: string): boolean => CHAIN.test(cmd);

/** True when the line carries the unquoted separator that aborts the script. */
export const isZshEqualsAbort = (cmd: string): boolean => ZSH_EQUALS.test(cmd);

/**
 * The benign filter that suppresses a row, or null to keep it. Bash only, run
 * after the named detectors have marked their rows.
 *
 * Three of the four require the shell's exit header, which no clean run carries
 * and no silent failure carries either — so suppression can never hide a row the
 * output scanner found. `user-rejected` is the exception: nothing ran, so there
 * is no exit status to read.
 *
 * The doc's own first wording is not what ships. `grep` exit 1 with empty output
 * fires on 2 rows of 961, and `chained-tail` scores 42% before the detector's
 * rows are removed and 71% after; `absence-probe` moves 77% to 91% the same way.
 */
export const benignFilter = (args: {
  readonly cmd: string;
  readonly output: string;
  readonly detector: string | null;
}): string | null => {
  const { cmd, output, detector } = args;
  // A row a detector claimed is never suppressed, however it was claimed.
  if (detector !== null || isZshEqualsAbort(cmd)) {
    return null;
  }
  if (USER_REJECTED.test(output)) {
    return "user-rejected";
  }
  if (!ranAndFailed(output)) {
    return null;
  }
  if (GLOB_NO_MATCH.test(output)) {
    return "glob-no-match";
  }
  const body = bodyOf(output);
  const tail = tailOf(cmd);
  const probes = ABSENCE_BINS.has(firstWord(tail)) || GUARDED_TAIL.test(tail);
  if (probes && (ABSENT_TEXT.test(output) || body.length === 0)) {
    return "absence-probe";
  }
  return isChain(cmd) &&
    output.length >= CHAINED_TAIL_MIN_CHARS &&
    !ERROR_TEXT.test(body)
    ? "chained-tail"
    : null;
};

/** Which family a fail row belongs to; each call is charged to exactly one. */
type FailKind = "detect" | "flag" | "scan";

/** The row a call is charged to, before it is folded. */
interface FailKey {
  readonly detector: string | null;
  readonly key: string;
  readonly kind: FailKind;
  readonly label: string;
}

/** Per-key state the `Tally` does not carry. Bounded by distinct keys. */
interface FailMeta {
  readonly detector: string | null;
  readonly kind: FailKind;
  readonly label: string;
  piped: number;
  readonly tools: Map<string, number>;
}

const labelForReason = (reason: string): string =>
  SIGNATURES.find((s) => s.id === reason)?.label ?? reason;

/**
 * The one row this call belongs to, or null when it is not a failure. Charging
 * is exclusive and ordered: a detector claims the row first, then the agent's
 * own flag, then the output scanner. A signature that matches two shapes is
 * charged to the first in declaration order — 2 rows of 1,070 in the corpus.
 */
const keyOf = (fact: CallFact): FailKey | null => {
  if (isBashFact(fact) && fact.failed && isZshEqualsAbort(fact.cmd)) {
    return {
      key: `detect:${ZSH_EQUALS_ABORT}`,
      label: "unquoted echo === separator",
      kind: "detect",
      detector: ZSH_EQUALS_ABORT,
    };
  }
  if (fact.err) {
    return fact.benign === null
      ? {
          key: `flag:${fact.tool}`,
          label: `${fact.tool} errors`,
          kind: "flag",
          detector: null,
        }
      : null;
  }
  if (fact.failed && fact.failReason !== null) {
    return {
      key: `scan:${fact.failReason}`,
      label: labelForReason(fact.failReason),
      kind: "scan",
      detector: null,
    };
  }
  return null;
};

/** The evidence line a row samples: the command, else the masked signature. */
const lineOf = (fact: CallFact): string => {
  if (isBashFact(fact)) {
    return fact.cmd;
  }
  return fact.sig ?? fact.arg ?? fact.tool;
};

const metaFor = (map: Map<string, FailMeta>, row: FailKey): FailMeta => {
  const found = map.get(row.key);
  if (found) {
    return found;
  }
  const fresh: FailMeta = {
    label: row.label,
    kind: row.kind,
    detector: row.detector,
    piped: 0,
    tools: new Map(),
  };
  map.set(row.key, fresh);
  return fresh;
};

/** The one-line fix a row carries, or null when the rule has none to give. */
const noteOf = (meta: FailMeta, rows: number): string | null => {
  if (meta.kind === "detect") {
    return 'quote the separator: echo "===" — zsh expands a bare === to a command name, exits 1, and never runs the rest of the line';
  }
  if (meta.kind !== "scan") {
    return null;
  }
  return meta.piped > 0
    ? `${meta.piped} of ${rows} pipe into head/tail/grep, so the exit code is the pipe's`
    : "the command printed a failure and still exited 0";
};

const detectorOf = (meta: FailMeta): string | null => {
  if (meta.detector !== null) {
    return meta.detector;
  }
  return meta.kind === "scan" && meta.piped > 0 ? PIPED_EXIT_CODE : null;
};

/** Drill hits read chronologically, with `sid` and `seq` breaking every tie. */
const compareHits = chain<DrillHit>(
  ascStr((h) => h.ts ?? ""),
  ascStr((h) => h.sid),
  asc((h) => h.seq)
);

/** The fail table: flagged rows and silent failures, clustered and ranked. */
export const makeFailTable: AccumulatorFactory<readonly FailRow[]> = (opts) => {
  const tallies = new Map<string, Tally>();
  const metas = new Map<string, FailMeta>();
  const hits: DrillHit[] = [];
  let erroredBash = 0;
  let suppressed = 0;

  const add = (fact: StatsFact): void => {
    if (!isCallFact(fact)) {
      return;
    }
    if (isBashFact(fact) && fact.err) {
      erroredBash += 1;
      if (fact.benign !== null) {
        suppressed += 1;
      }
    }
    const row = keyOf(fact);
    if (row === null) {
      return;
    }
    const line = opts.redact.line(lineOf(fact));
    addToTally({
      tally: tallyFor(tallies, row.key),
      fact,
      sample: sampleOf(fact, line),
    });
    const meta = metaFor(metas, row);
    bump(meta.tools, fact.tool);
    if (isBashFact(fact) && fact.piped) {
      meta.piped += 1;
    }
    if (opts.drill?.table === "fail" && opts.drill.key === row.key) {
      hits.push(hitOf(fact, line));
    }
  };

  const rowOf = (args: {
    readonly ctx: FinishContext;
    readonly key: string;
    readonly meta: FailMeta;
    readonly tally: Tally;
  }): FailRow => {
    const { ctx, key, meta, tally } = args;
    const note = noteOf(meta, tally.rows);
    const tool = topKey(meta.tools);
    return {
      key,
      label: ctx.redact.line(meta.label),
      where: meta.kind === "scan" ? `${tool} exit 0` : tool,
      rows: tally.rows,
      sessions: tally.sessions.size,
      projects: tally.projects.size,
      flagged: tally.flagged,
      note: note === null ? null : ctx.redact.line(note),
      detector: detectorOf(meta),
      samples: samplesOf(tally, ctx.sampleCap),
    };
  };

  const finish = (ctx: FinishContext): readonly FailRow[] => {
    const rows = [...tallies.entries()].flatMap(([key, tally]) => {
      const meta = metas.get(key);
      return meta ? [rowOf({ ctx, key, meta, tally })] : [];
    });
    const ranked = rank(rows, compareFailRows, ctx.limit);
    if (ranked.dropped > 0) {
      ctx.caveat(`fail table: ${ranked.dropped} rows not shown`);
    }
    if (suppressed > 0) {
      const pct = round(share(suppressed, erroredBash) * PERCENT, 1);
      ctx.caveat(
        `${suppressed} of ${erroredBash} errored bash rows (${pct}%) are suppressed as benign, at a measured 78% precision`
      );
    }
    return ranked.rows;
  };

  return {
    id: "fails",
    add,
    finish,
    drill: () => sortBy(hits, compareHits),
  };
};
