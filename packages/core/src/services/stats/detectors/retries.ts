/**
 * The cause table: every catalog cause, what it cost in retries, and its fix.
 *
 * Hits come straight off the facts (`cause`, set at extract time). Retry pairs
 * come from the pairing engine in `pairing.ts`. Per-1k rates divide by the
 * calls that could have hit: shell calls for a shell cause, else calls of the
 * tools the cause watches. Rates and the "rule present but still firing" check
 * need counts on whole days after a memory file changed, in the calls that
 * file reaches, so hits and denominators are also kept per UTC day, agent,
 * project and working dir — bounded by the window and the dirs sessions ran
 * in, not by rows. The dir is the session's own: a workflow agent or subagent
 * stored under its launcher's project often runs in another repo, and reads
 * that repo's memory files.
 *
 * Seconds are the call-to-call gap of each pair; tokens are the context the
 * retry's model call re-read, counted once per model call. Neither is folded
 * into the other or into a score.
 */

import type { AgentId } from "../../agent-id";
import {
  type AccumulatorFactory,
  type FactAccumulator,
  type FinishContext,
  hitOf,
  sampleOf,
} from "../accumulate";
import {
  asc,
  ascStr,
  boundPool,
  bump,
  type Compare,
  chain,
  clip,
  cmpStr,
  collapseWs,
  compareSamples,
  desc,
  pickSamples,
  rank,
  round,
  SAMPLE_LINE_MAX,
  safeDiv,
  sortBy,
} from "../rank";
import {
  type CallFact,
  type CauseDetail,
  type CauseDiff,
  type CauseId,
  type CauseRow,
  type DrillHit,
  isBashFact,
  isCallFact,
  type PairSample,
  type SampleRef,
  type Split,
  type StatsFact,
} from "../schema";
import { binName, commandIndex, splitSegments, tokenize } from "../shell-lex";
import { CAUSES, GIT_DIFF_OUTPUT, globValue, PATH_FILES } from "./causes";
import { hitCauseOf, makePairer, type RetryPair } from "./pairing";
import type { CauseCounts, CoverSince, RuleCover } from "./rules";

/** Culprit values a row lists. */
const DETAIL_CAP = 5;

/** Learned diffs a row lists. */
const DIFF_CAP = 3;

/** Binaries a version manager or shell profile puts on PATH. */
const PATH_BINS: ReadonlySet<string> = new Set([
  "node",
  "npm",
  "npx",
  "pnpm",
  "yarn",
  "fnm",
  "bun",
  "bunx",
  "deno",
  "tsx",
  "corepack",
  "cargo",
  "rustc",
  "go",
]);

/** Package that ships a binary, when its name differs. */
const PACKAGE_OF: Readonly<Record<string, string>> = {
  strings: "binutils (strings)",
  objdump: "binutils (objdump)",
  readelf: "binutils (readelf)",
  rg: "ripgrep (rg)",
  // Debian/Ubuntu's fd-find installs `fdfind`, so `fd` needs a link too, in a
  // dir that may not exist yet; `commandNotFoundFix` puts it on PATH.
  fd: "fd (on Debian/Ubuntu: apt install fd-find, then mkdir -p ~/.local/bin && ln -s $(command -v fdfind) ~/.local/bin/fd)",
  xxd: "xxd",
  bc: "bc",
};

/** Chars of a fix line a row keeps. */
export const FIX_MAX = 400;

/** Chars of a detail value or a diff side a row keeps. */
const VALUE_MAX = 80;

/** Bin-composed fixes name at most this many binaries. */
const FIX_BINS = 4;

const PER_MILLE = 1000;

const SEP = "\u0000";

/** A split with both sides zero. */
const zero = (): { main: number; nested: number } => ({ main: 0, nested: 0 });

const addTo = (split: { main: number; nested: number }, nested: boolean) => {
  if (nested) {
    split.nested += 1;
  } else {
    split.main += 1;
  }
};

/** A sidechain inside a main transcript is a subagent too. */
const isNested = (fact: CallFact): boolean => fact.kind !== "main" || fact.side;

/** UTC day of a call, or "" when it has no time. */
const dayOf = (fact: CallFact): string => fact.ts?.slice(0, 10) ?? "";

/** Where a session ran: its cwd and the agent source it is stored in. */
interface Origin {
  /** The session's own cwd, or null when its session row was not seen. */
  readonly cwd: string | null;
  /** The agent source, or null for the default or an unseen session row. */
  readonly source: string | null;
}

const NO_ORIGIN: Origin = { cwd: null, source: null };

/** Counts of one UTC day, agent, project, working dir and source. */
interface Cell extends Origin {
  readonly agent: AgentId;
  readonly day: string;
  main: number;
  nested: number;
  readonly project: string;
}

/** Cells of one series, keyed by day, agent, project, cwd and source. */
type CellSeries = Map<string, Cell>;

const bumpCell = (series: CellSeries, fact: CallFact, origin: Origin): void => {
  const day = dayOf(fact);
  const key = [
    day,
    fact.agent,
    fact.project,
    origin.cwd ?? "",
    origin.source ?? "",
  ].join(SEP);
  const found = series.get(key);
  if (found) {
    addTo(found, isNested(fact));
    return;
  }
  const fresh: Cell = {
    day,
    agent: fact.agent,
    project: fact.project,
    cwd: origin.cwd,
    source: origin.source,
    main: 0,
    nested: 0,
  };
  addTo(fresh, isNested(fact));
  series.set(key, fresh);
};

/** Which cells to sum: all of them, or those some cover reaches in time. */
type CellFilter = (cell: Cell) => boolean;

const ALL_CELLS: CellFilter = () => true;

/** Sum the cells of some series that pass the filter. */
const sumCells = (series: readonly CellSeries[], keep: CellFilter): Split => {
  const out = zero();
  for (const s of series) {
    for (const cell of s.values()) {
      if (keep(cell)) {
        out.main += cell.main;
        out.nested += cell.nested;
      }
    }
  }
  return out;
};

/** Hits per thousand calls, per side. */
export const per1kOf = (hits: Split, calls: Split): Split => ({
  main: round(safeDiv(hits.main * PER_MILLE, calls.main)),
  nested: round(safeDiv(hits.nested * PER_MILLE, calls.nested)),
});

/** Per-cause state. Bounded by distinct culprits, diffs, days and projects. */
interface CauseAgg {
  readonly cause: CauseId;
  readonly ctxSeen: Set<string>;
  readonly details: Map<string, number>;
  readonly diffs: Map<string, number>;
  flagged: number;
  /** Hits on a `git diff` family, for the output-too-large fix. */
  gitDiffHits: number;
  readonly hitsByCell: CellSeries;
  pairSamples: PairSample[];
  pairs: number;
  readonly pairsBy: { main: number; nested: number };
  retryCtxTokens: number;
  retrySec: number;
  readonly retrySessions: Set<string>;
  samples: SampleRef[];
  secMeasured: number;
  readonly sessions: Set<string>;
  tokensMeasured: number;
  /** For an unclassified row, the tool it groups. */
  readonly tool: string | null;
}

/** Hits of a cause, per side, summed over its cells. */
const hitsOf = (agg: CauseAgg): Split => sumCells([agg.hitsByCell], ALL_CELLS);

const freshAgg = (cause: CauseId, tool: string | null): CauseAgg => ({
  cause,
  tool,
  flagged: 0,
  gitDiffHits: 0,
  hitsByCell: new Map(),
  pairs: 0,
  pairsBy: zero(),
  retrySessions: new Set(),
  sessions: new Set(),
  retrySec: 0,
  secMeasured: 0,
  retryCtxTokens: 0,
  tokensMeasured: 0,
  ctxSeen: new Set(),
  details: new Map(),
  diffs: new Map(),
  samples: [],
  pairSamples: [],
});

/** Row key of a cause; unclassified rows split by tool. */
const causeKeyOf = (cause: CauseId, tool: string): string =>
  cause === "unclassified" ? `cause:unclassified:${tool}` : `cause:${cause}`;

/** The evidence line a cause samples: the command, else the signature. */
const lineOf = (fact: CallFact): string => {
  if (isBashFact(fact)) {
    return fact.cmd;
  }
  return fact.sig ?? fact.arg ?? fact.tool;
};

/** Chars of a command family a persisted-output detail keeps. */
const FAMILY_MAX = 40;

/**
 * The culprit as the details list counts it: a flag glob loses its flag, and a
 * persisted output is counted by the command family that printed it (its saved
 * file name is unique per call).
 */
const detailOf = (cause: CauseId, fact: CallFact): string | null => {
  if (cause === "output-too-large") {
    return isBashFact(fact) ? fact.family.slice(0, FAMILY_MAX) : fact.tool;
  }
  if (fact.causeArg === null) {
    return null;
  }
  return cause === "zsh-glob-flag" ? globValue(fact.causeArg) : fact.causeArg;
};

/** A sample as it ships: its line redacted once more, as every minted string. */
const redactSample = (ctx: FinishContext, sample: SampleRef): SampleRef => ({
  ...sample,
  line: ctx.redact.line(sample.line),
});

/** Pairs read in the order of their failed calls' samples. */
const comparePairSamples: Compare<PairSample> = (a, b) =>
  compareSamples(a.failed, b.failed);

const pickPairSamples = (
  pool: readonly PairSample[],
  cap: number
): readonly PairSample[] => sortBy(pool, comparePairSamples).slice(0, cap);

/** Cause rows: retry sessions, then sessions, then pairs, then key. Never a score. */
const compareCauseRows = chain<CauseRow>(
  desc((r) => r.retrySessions),
  desc((r) => r.sessions),
  desc((r) => r.pairs),
  ascStr((r) => r.key)
);

const topCounts = (counts: Map<string, number>, cap: number) =>
  sortBy(
    [...counts.entries()],
    chain<[string, number]>(
      desc((e) => e[1]),
      ascStr((e) => e[0])
    )
  ).slice(0, cap);

/** Where a package fix links a binary the package names differently. */
const LOCAL_BIN = "~/.local/bin";
const LINKED_INTO_LOCAL_BIN: ReadonlySet<string> = new Set(["fd"]);

/**
 * Names that are shell functions a profile sources, not executables: no PATH
 * entry can bring them back, so the fix says what to source instead.
 */
const SOURCED_FIX: Readonly<Record<string, string>> = {
  nvm: `nvm is a shell function, not a binary: source $NVM_DIR/nvm.sh in ${PATH_FILES}, or put a node version's bin dir on PATH instead`,
};

/** "install jq, binutils (strings); or put node on PATH in ~/.zshenv …". */
const commandNotFoundFix = (bins: readonly string[]): string | null => {
  const top = bins.slice(0, FIX_BINS);
  const sourced = top.flatMap((b) => SOURCED_FIX[b] ?? []);
  const install = top.filter((b) => !(PATH_BINS.has(b) || b in SOURCED_FIX));
  // A non-login agent shell may not have the link dir on PATH.
  const linked = install.some((b) => LINKED_INTO_LOCAL_BIN.has(b));
  const onPath = [
    ...top.filter((b) => PATH_BINS.has(b)),
    ...(linked ? [LOCAL_BIN] : []),
  ];
  const parts: string[] = [];
  if (install.length > 0) {
    parts.push(`install ${install.map((b) => PACKAGE_OF[b] ?? b).join(", ")}`);
  }
  if (onPath.length > 0) {
    parts.push(`put ${onPath.join(" and ")} on PATH in ${PATH_FILES}`);
  }
  const all = [
    ...(parts.length === 0 ? [] : [parts.join(linked ? "; then " : "; or ")]),
    ...sourced,
  ];
  return all.length === 0 ? null : all.join("; ");
};

/**
 * The fix one call prints, sharpened by its own culprit the way its row is:
 * the transcript marker and the cause row then name the same remedy.
 */
export const callFixOf = (
  cause: CauseId,
  arg: string | null
): string | null => {
  const spec = CAUSES[cause];
  if (arg === null) {
    return spec.fix;
  }
  if (cause === "command-not-found") {
    return commandNotFoundFix([arg]) ?? spec.fix;
  }
  if (cause === "structured-output-schema") {
    return `${spec.fix} Missing: '${arg}'.`;
  }
  return spec.fix;
};

/**
 * The fix a row prints, sharpened by what its hits named. A missing binary
 * that a later call ran, or that the host has now, is installed: the fix
 * skips it, and a row whose every binary is installed has nothing to fix.
 */
const fixOf = (
  agg: CauseAgg,
  installed: (bin: string) => boolean
): string | null => {
  const spec = CAUSES[agg.cause];
  if (agg.cause === "command-not-found") {
    const bins = topCounts(agg.details, agg.details.size).map(([b]) => b);
    const missing = bins.filter((b) => !installed(b)).slice(0, FIX_BINS);
    if (bins.length > 0 && missing.length === 0) {
      return null;
    }
    return commandNotFoundFix(missing) ?? spec.fix;
  }
  if (agg.cause === "output-too-large") {
    const hits = hitsOf(agg);
    return agg.gitDiffHits * 2 >= hits.main + hits.nested
      ? GIT_DIFF_OUTPUT.fix
      : spec.fix;
  }
  if (agg.cause === "structured-output-schema") {
    const [top] = topCounts(agg.details, 1);
    return top ? `${spec.fix} Most often missing: '${top[0]}'.` : spec.fix;
  }
  return spec.fix;
};

/** The binaries a command runs, one per top-level segment, by basename. */
const ranBins = (cmd: string): readonly string[] =>
  splitSegments(cmd).segments.flatMap((segment) => {
    const words = tokenize(segment).map((w) => w.text);
    const word = words[commandIndex(words)];
    return word === undefined ? [] : [binName(word)];
  });

/** Keep the later of `at`'s current value and the call's time. */
const latest = (at: Map<string, number>, bin: string, fact: CallFact): void => {
  const ms = fact.ts === null ? Number.NaN : Date.parse(fact.ts);
  if (!Number.isNaN(ms) && ms > (at.get(bin) ?? Number.NEGATIVE_INFINITY)) {
    at.set(bin, ms);
  }
};

/** Denominator of a cause, per cell, read from the shared per-tool series. */
interface Denominators {
  readonly shell: CellSeries;
  readonly tools: Map<string, CellSeries>;
}

const denominatorOf = (agg: CauseAgg, den: Denominators): CellSeries[] => {
  const per = CAUSES[agg.cause].per;
  if (per === "shell") {
    return [den.shell];
  }
  const rowTools = agg.tool === null ? [] : [agg.tool];
  const tools = per === "row-tool" ? rowTools : per;
  return tools.flatMap((t) => {
    const series = den.tools.get(t);
    return series ? [series] : [];
  });
};

const BACKSLASH = /\\/g;

/** True when `cwd` is `dir` or sits under it, with either path separator. */
const isUnder = (cwd: string, dir: string): boolean => {
  const path = cwd.replace(BACKSLASH, "/");
  const slashed = dir.replace(BACKSLASH, "/");
  const base = slashed.endsWith("/") ? slashed.slice(0, -1) : slashed;
  return path === base || path.startsWith(`${base}/`);
};

/** True when a cover's source, if it names one, stored the cell's session. */
const fromSource = (cover: RuleCover, cell: Cell): boolean =>
  cover.source === undefined ||
  (cell.source === null
    ? cover.source.isDefault
    : cell.source === cover.source.id);

/** True when a project file in `dir` is read by a call made in `cwd`. */
const walkReaches = (cover: RuleCover, dir: string, cwd: string): boolean => {
  if (cover.codex === undefined) {
    return isUnder(cwd, dir);
  }
  if (cover.codex.exact) {
    return isUnder(cwd, dir) && isUnder(dir, cwd);
  }
  return isUnder(cwd, dir) && !cover.codex.stops.some((s) => isUnder(cwd, s));
};

/** What the cause table yields: the ranked rows and the counts behind them. */
interface CauseTableResult {
  readonly counts: CauseCounts;
  readonly rows: readonly CauseRow[];
}

/** The cause table, plus the project dirs whose memory files a rule check reads. */
export interface CauseTable extends FactAccumulator<CauseTableResult> {
  /**
   * Binaries the host has now, read before `finish`: the fix stops naming
   * them. A missed binary that a later call ran is dropped without it.
   */
  readonly markInstalled: (bins: Iterable<string>) => void;
  /** Binaries a not-found named that no later call ran, most hits first. */
  readonly missingBins: () => readonly string[];
  /** The working dirs each cause hit most in, sorted, unique. */
  readonly projectDirs: () => readonly string[];
}

/** Working dirs per cause whose memory files are read. */
const RULE_DIRS = 5;

/** Build the cause table. */
export const makeCauseTable: (
  opts: Parameters<AccumulatorFactory<CauseTableResult>>[0]
) => CauseTable = (opts) => {
  const aggs = new Map<string, CauseAgg>();
  const den: Denominators = { shell: new Map(), tools: new Map() };
  /** Each session's own cwd and source, from its session row. */
  const sessionOrigin = new Map<string, Origin>();
  /** A project's smallest session cwd: the dir of a call whose row was not seen. */
  const projectCwd = new Map<string, string>();
  const drillHits = new Map<string, DrillHit>();
  const drillKey = opts.drill?.table === "cause" ? opts.drill.key : null;
  /** Per binary, the latest call that found it missing, in epoch ms. */
  const lastMiss = new Map<string, number>();
  /** Per binary, the latest call that ran it with no not-found, in epoch ms. */
  const lastRan = new Map<string, number>();
  /** Binaries the host has now, from `markInstalled`. */
  const onHost = new Set<string>();
  /** True when `bin` ran after the last call that found it missing. */
  const ranSince = (name: string): boolean => {
    const ran = lastRan.get(name);
    const miss = lastMiss.get(name);
    return ran !== undefined && miss !== undefined && ran > miss;
  };
  /** True when `bin` is installed now: it ran since, or the host has it. */
  const installed = (bin: string): boolean => {
    const name = binName(bin);
    return ranSince(name) || onHost.has(name);
  };

  const aggFor = (cause: CauseId, tool: string): CauseAgg => {
    const key = causeKeyOf(cause, tool);
    const found = aggs.get(key);
    if (found) {
      return found;
    }
    const fresh = freshAgg(cause, cause === "unclassified" ? tool : null);
    aggs.set(key, fresh);
    return fresh;
  };

  const onPair = (pair: RetryPair): void => {
    const agg = aggFor(pair.cause, pair.failed.tool);
    const { failed, retry } = pair;
    agg.pairs += 1;
    addTo(agg.pairsBy, isNested(failed));
    agg.retrySessions.add(failed.sid);
    if (pair.gapSec !== null) {
      agg.retrySec += pair.gapSec;
      agg.secMeasured += 1;
    }
    if (retry.ctxTok !== null) {
      agg.tokensMeasured += 1;
      const id = `${retry.sid}${SEP}${retry.req ?? retry.seq}`;
      if (!agg.ctxSeen.has(id)) {
        agg.ctxSeen.add(id);
        agg.retryCtxTokens += retry.ctxTok;
      }
    }
    if (pair.diff !== null) {
      bump(agg.diffs, `${pair.diff.from}${SEP}${pair.diff.to}`);
    }
    // Fact text is redacted at extract time; lines are collapsed and clipped
    // here and re-redacted in `finish`, for the few samples that ship.
    const failedLine = clip(collapseWs(lineOf(failed)), SAMPLE_LINE_MAX);
    const retryLine = clip(collapseWs(lineOf(retry)), SAMPLE_LINE_MAX);
    agg.pairSamples.push({
      failed: sampleOf(failed, failedLine),
      retry: sampleOf(retry, retryLine),
    });
    agg.pairSamples = boundPool(agg.pairSamples, pickPairSamples);
    if (drillKey === causeKeyOf(pair.cause, failed.tool)) {
      const id = `${failed.sid}#${failed.seq}`;
      const hit = drillHits.get(id);
      if (hit) {
        drillHits.set(id, {
          ...hit,
          pairToolUseId: retry.toolUseId,
          pairLine: retryLine,
        });
      }
    }
  };

  const pairer = makePairer(onPair);

  /** The dir a cell was counted under, falling back to its project's. */
  const cwdOfCell = (cell: Cell): string | undefined =>
    cell.cwd ?? projectCwd.get(cell.project);

  const addDenominator = (fact: CallFact, origin: Origin): void => {
    if (isBashFact(fact)) {
      bumpCell(den.shell, fact, origin);
    }
    const series = den.tools.get(fact.tool) ?? new Map();
    bumpCell(series, fact, origin);
    den.tools.set(fact.tool, series);
  };

  /** A shell call that ran with no not-found proves its binaries are there. */
  const noteRan = (fact: CallFact, cause: CauseId | null): void => {
    if (isBashFact(fact) && !fact.err && cause !== "command-not-found") {
      for (const bin of ranBins(fact.cmd)) {
        latest(lastRan, bin, fact);
      }
    }
  };

  /** Count one hit: a named cause on the fact, or an unexplained error. */
  const addHit = (fact: CallFact, cause: CauseId, origin: Origin): void => {
    const agg = aggFor(cause, fact.tool);
    if (fact.err) {
      agg.flagged += 1;
    }
    bumpCell(agg.hitsByCell, fact, origin);
    agg.sessions.add(fact.sid);
    const detail = detailOf(cause, fact);
    if (detail !== null) {
      bump(agg.details, detail);
      if (cause === "command-not-found") {
        latest(lastMiss, binName(detail), fact);
      }
    }
    if (isBashFact(fact) && fact.family.startsWith("git diff")) {
      agg.gitDiffHits += 1;
    }
    const line = clip(collapseWs(lineOf(fact)), SAMPLE_LINE_MAX);
    agg.samples.push(sampleOf(fact, line));
    agg.samples = boundPool(agg.samples, pickSamples);
    if (drillKey === causeKeyOf(cause, fact.tool)) {
      drillHits.set(`${fact.sid}#${fact.seq}`, hitOf(fact, line));
    }
  };

  const add = (fact: StatsFact): void => {
    if (fact.row === "session") {
      sessionOrigin.set(fact.sid, { cwd: fact.cwd, source: fact.source });
      if (fact.cwd !== null) {
        const known = projectCwd.get(fact.project);
        // Smallest wins, so the fallback never depends on walk order.
        if (known === undefined || cmpStr(fact.cwd, known) < 0) {
          projectCwd.set(fact.project, fact.cwd);
        }
      }
      return;
    }
    if (!isCallFact(fact)) {
      return;
    }
    // A transcript's session row comes first, unless the window dropped it.
    const origin = sessionOrigin.get(fact.sid) ?? NO_ORIGIN;
    addDenominator(fact, origin);
    const cause = hitCauseOf(fact);
    if (cause !== null) {
      addHit(fact, cause, origin);
    }
    noteRan(fact, cause);
    pairer.step(fact);
  };

  const rowOf = (ctx: FinishContext, key: string, agg: CauseAgg): CauseRow => {
    const spec = CAUSES[agg.cause];
    const fix = fixOf(agg, installed);
    const hitsBy = hitsOf(agg);
    return {
      key,
      cause: agg.cause,
      kind: spec.kind,
      label: ctx.redact.line(
        agg.tool === null ? spec.label : `${agg.tool} errored, no named cause`
      ),
      fix: fix === null ? null : ctx.redact.line(fix, FIX_MAX),
      failing: spec.failing,
      hits: hitsBy.main + hitsBy.nested,
      flagged: agg.flagged,
      hitsBy,
      pairs: agg.pairs,
      pairsBy: { ...agg.pairsBy },
      retrySessions: agg.retrySessions.size,
      sessions: agg.sessions.size,
      projects: new Set([...agg.hitsByCell.values()].map((c) => c.project))
        .size,
      retrySec: round(agg.retrySec),
      secMeasured: agg.secMeasured,
      retryCtxTokens: agg.retryCtxTokens,
      tokensMeasured: agg.tokensMeasured,
      per1k: per1kOf(hitsBy, sumCells(denominatorOf(agg, den), ALL_CELLS)),
      details: topCounts(agg.details, DETAIL_CAP).map(
        ([value, hits]): CauseDetail => ({
          value: ctx.redact.line(value, VALUE_MAX),
          hits,
        })
      ),
      diffs: topCounts(agg.diffs, DIFF_CAP).map(([pair, pairs]): CauseDiff => {
        const [from = "", to = ""] = pair.split(SEP);
        return {
          from: ctx.redact.line(from, VALUE_MAX),
          to: ctx.redact.line(to, VALUE_MAX),
          pairs,
        };
      }),
      rule: null,
      samples: pickSamples(agg.samples, ctx.sampleCap).map((sample) =>
        redactSample(ctx, sample)
      ),
      pairSamples: pickPairSamples(agg.pairSamples, ctx.sampleCap).map((p) => ({
        failed: redactSample(ctx, p.failed),
        retry: redactSample(ctx, p.retry),
      })),
    };
  };

  /** Unclassified rows only matter where a retry was paid. */
  const shown = (agg: CauseAgg): boolean =>
    agg.cause !== "unclassified" || agg.pairs > 0;

  let flushed = false;
  const settle = (): void => {
    if (!flushed) {
      pairer.flush();
      flushed = true;
    }
  };

  const finish = (ctx: FinishContext): CauseTableResult => {
    settle();
    const rows = [...aggs.entries()]
      .filter(([, agg]) => shown(agg))
      .map(([key, agg]) => rowOf(ctx, key, agg));
    const ranked = rank(rows, compareCauseRows, ctx.limit);
    if (ranked.dropped > 0) {
      ctx.caveat(`cause table: ${ranked.dropped} rows not shown`);
    }
    const reaches = (cover: RuleCover, cell: Cell): boolean => {
      if (cell.agent !== cover.agent || !fromSource(cover, cell)) {
        return false;
      }
      if (cover.dir === null) {
        return true;
      }
      const cwd = cwdOfCell(cell);
      return cwd !== undefined && walkReaches(cover, cover.dir, cwd);
    };
    const filterOf =
      (covers: readonly CoverSince[]): CellFilter =>
      (cell) =>
        covers.some(
          (c) =>
            reaches(c.cover, cell) &&
            (c.afterDay === "" || (cell.day !== "" && cell.day > c.afterDay))
        );
    const counts: CauseCounts = {
      since: (key, covers) => {
        const agg = aggs.get(key);
        if (!agg) {
          return { hits: zero(), calls: zero() };
        }
        const keep = filterOf(covers);
        return {
          hits: sumCells([agg.hitsByCell], keep),
          calls: sumCells(denominatorOf(agg, den), keep),
        };
      },
    };
    return { rows: ranked.rows, counts };
  };

  const projectDirs = (): readonly string[] => {
    const dirs = new Set<string>();
    for (const agg of aggs.values()) {
      if (agg.cause === "unclassified") {
        continue;
      }
      const byDir = new Map<string, number>();
      for (const cell of agg.hitsByCell.values()) {
        const cwd = cwdOfCell(cell);
        if (cwd !== undefined) {
          bump(byDir, cwd, cell.main + cell.nested);
        }
      }
      for (const [cwd] of topCounts(byDir, RULE_DIRS)) {
        dirs.add(cwd);
      }
    }
    return [...dirs].sort(cmpStr);
  };

  const drill = (): readonly DrillHit[] => {
    settle();
    return sortBy(
      [...drillHits.values()].map((hit) => ({
        ...hit,
        line: opts.redact.line(hit.line),
        pairLine: hit.pairLine === null ? null : opts.redact.line(hit.pairLine),
      })),
      chain<DrillHit>(
        asc((h) => (h.pairLine === null ? 1 : 0)),
        ascStr((h) => h.ts ?? ""),
        ascStr((h) => h.sid),
        asc((h) => h.seq)
      )
    );
  };

  const missingBins = (): readonly string[] =>
    sortBy(
      [...lastMiss.keys()].filter((bin) => !ranSince(bin)),
      chain<string>(
        desc((bin) => missCount(bin)),
        ascStr((bin) => bin)
      )
    );

  /** Not-found hits that named `bin`, over the row's details. */
  const missCount = (bin: string): number => {
    let n = 0;
    for (const [value, hits] of aggs.get("cause:command-not-found")?.details ??
      []) {
      n += binName(value) === bin ? hits : 0;
    }
    return n;
  };

  const markInstalled = (bins: Iterable<string>): void => {
    for (const bin of bins) {
      onHost.add(bin);
    }
  };

  return {
    id: "causes",
    add,
    finish,
    drill,
    markInstalled,
    missingBins,
    projectDirs,
  };
};
