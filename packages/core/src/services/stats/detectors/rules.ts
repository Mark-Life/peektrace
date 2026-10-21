/**
 * Rule present but still firing.
 *
 * Each cause carries keyword probes (`CauseSpec.probes`). A memory-file line
 * (or a three-line window around it) that satisfies every regex of one probe
 * is read as "the rule is already there", for the calls that file reaches: a
 * global Claude file the sessions of its own Claude source, Codex's file every
 * Codex call, and a project file the calls made under its directory by the
 * agent that loads that file name. A file that reaches none of a cause's hits
 * does not count. If the cause keeps firing, in the calls those files reach,
 * on whole days after the matched lines appeared, another memory line will not
 * help: the row says so, with the rate and the main-vs-subagent split, and
 * names an env or prompt-level fix.
 *
 * A line is dated by when it appeared (`LineDater`), not by its file's mtime:
 * editing one line of a file must not reset the rules its other lines hold.
 *
 * A probe is a keyword test, not a reading: a match means a line talks about
 * the habit, which is what the caveat says.
 */
import type { AgentId } from "../../agent-id";
import { desc, safeDiv, sortBy } from "../rank";
import type { Redactor } from "../redact";
import type { CauseId, CauseRow, RuleCheck, RuleMatch, Split } from "../schema";
import { CAUSES, escapeRe, GIT_DIFF_OUTPUT, type RuleProbe } from "./causes";
import { per1kOf } from "./retries";

/**
 * The calls a memory file reaches: every call of the agent that loads it, or,
 * for a project file, only that agent's calls made under its directory.
 */
export interface RuleCover {
  readonly agent: AgentId;
  /**
   * Codex's walk, for a project file. Codex reads from the cwd up to the
   * nearest repo root: `exact` when `dir` has no root above it, so only a call
   * made in `dir` itself reads it; else every cwd under `dir` but those inside
   * a nested repo root (`stops`), whose walk ends there.
   */
  readonly codex?: {
    readonly exact: boolean;
    readonly stops: readonly string[];
  };
  readonly dir: string | null;
  /**
   * The agent source whose sessions load this file, when the agent has more
   * than one: a global `CLAUDE.md` of `~/.claude-work` reaches only the
   * sessions stored there. A session with no source recorded is the default's.
   */
  readonly source?: { readonly id: string; readonly isDefault: boolean };
}

/**
 * One memory file, split into lines for the probes. Read by `memory-files.ts`;
 * declared here so a browser bundle can match rules without the FS layer.
 */
export interface MemoryDoc {
  readonly cover: RuleCover;
  /** `~`-collapsed path, with ` → target` when it is a symlink. */
  readonly displayPath: string;
  readonly lines: readonly string[];
  readonly mtimeMs: number | null;
  readonly realPath: string;
}

/**
 * When line `line` (0-based) of a doc appeared, in epoch ms, or null when
 * unknown (every hit counts). `rule-dates.ts` reads it from a ledger of
 * first-seen lines; `byMtime` dates every line by its file's mtime.
 */
export type LineDater = (doc: MemoryDoc, line: number) => number | null;

/** Every line dated by its file's mtime: for a caller with no line history. */
export const byMtime: LineDater = (doc) => doc.mtimeMs;

/** One file's reach, and the UTC day after which its hits count ("" = all). */
export interface CoverSince {
  readonly afterDay: string;
  readonly cover: RuleCover;
}

/** What the rule check reads back from the cause table after the pass. */
export interface CauseCounts {
  /**
   * Hits and denominator calls of one row, counting a call when some cover
   * reaches it on a whole day strictly after that cover's `afterDay`.
   */
  readonly since: (
    key: string,
    covers: readonly CoverSince[]
  ) => { readonly hits: Split; readonly calls: Split };
}

/**
 * Probes for a missing binary: a line that says this binary is absent or
 * replaced. A bare name is not enough: `file` and `node` are English words, so
 * "no file" counts only as "has no file", "there is no file", "no `file`" or
 * "no file binary" (command, executable, installed, on PATH), never in prose
 * like "finds no file, say so". The name is a whole word: "install files" and
 * "instead of node_modules" are not about `file` or `node`.
 */
const binaryProbes = (row: CauseRow): readonly RuleProbe[] =>
  row.details.map((d) => {
    const name = escapeRe(d.value);
    const start = "(?<![\\w.-])";
    // A word end: "file." ends a sentence, "file.txt" is another word.
    const end = "(?=\\s|$|[,;:)]|\\.(?:\\s|$))";
    const b = `${start}\`?${name}\`?${end}`;
    const absent = [
      `\\b(?:has|have|there is|there's|there are) no ${b}`,
      `\\bno \`${name}\`${end}`,
      `\\bno ${b} (?:binary|command|executable|installed|on (?:this )?(?:host|machine|box|PATH))\\b`,
    ].join("|");
    return [
      new RegExp(
        `${absent}|${b} (?:is )?(?:not installed|missing|unavailable|not available|not on PATH)|\\b(?:install|without|instead of) ${b}`,
        "i"
      ),
    ];
  });

/**
 * A row's probes. An output-too-large row whose fix is the `git diff` one also
 * accepts a `git diff` line; any other needs a rule that bounds all output.
 */
const probesOf = (row: CauseRow): readonly RuleProbe[] => {
  if (row.cause === "command-not-found") {
    return binaryProbes(row);
  }
  const { probes } = CAUSES[row.cause];
  return row.cause === "output-too-large" && row.fix === GIT_DIFF_OUTPUT.fix
    ? [...probes, ...GIT_DIFF_OUTPUT.probes]
    : probes;
};

/** Matches a row lists. */
const MATCH_CAP = 3;

/** Chars of a matched line a row prints. */
const EXCERPT_MAX = 160;

/** Share of hits from nested agents above which the prompt is the place. */
const NESTED_SHARE = 0.8;

const PERCENT = 100;

/** Chars of an advice line a row keeps. */
const ADVICE_MAX = 400;

const total = (s: Split): number => s.main + s.nested;

/**
 * Lines of one doc a probe accepts. A line that satisfies a whole probe on its
 * own wins; only a doc with none falls back to a ±1-line window, whose first
 * line is reported.
 */
const matchDoc = (doc: MemoryDoc, probes: readonly RuleProbe[]): number[] => {
  const accepts = (text: string) =>
    probes.some((probe) => probe.every((re) => re.test(text)));
  const single = doc.lines.flatMap((line, i) => (accepts(line) ? [i] : []));
  if (single.length > 0) {
    return single;
  }
  const hits: number[] = [];
  for (let i = 0; i < doc.lines.length; i++) {
    const line = doc.lines[i] ?? "";
    const window = doc.lines.slice(Math.max(0, i - 1), i + 2).join("\n");
    const anchored = probes.some(
      (probe) =>
        (probe[0]?.test(line) ?? false) && probe.every((re) => re.test(window))
    );
    if (anchored && hits.at(-1) !== i - 1) {
      hits.push(i);
    }
  }
  return hits;
};

const dayIso = (ms: number): string => new Date(ms).toISOString().slice(0, 10);

/**
 * What a still-firing rule should do instead of another memory line. A rule
 * line added on the window's last day has no whole day after it yet: the advice
 * then says what to do if the cause keeps firing.
 */
const adviceOf = (cause: CauseId, hits: Split, calls: Split): string => {
  const base =
    CAUSES[cause].advice ??
    "move the rule into the prompt that keeps hitting it";
  if (total(calls) === 0) {
    return `if it keeps firing: ${base}`;
  }
  const n = total(hits);
  if (n === 0) {
    return "no hits on whole days since the rule appeared";
  }
  const nested = safeDiv(hits.nested, n);
  return nested >= NESTED_SHARE
    ? `${base}. ${Math.round(nested * PERCENT)}% of hits come from subagents or workflow agents, which may not load your memory file: put the rule in the agent or workflow prompt`
    : base;
};

/**
 * A doc whose lines state the rule, the lines that do, and since when: its
 * oldest matched line's date, or null when one of them has none.
 */
interface DocMatch {
  readonly doc: MemoryDoc;
  readonly lines: readonly number[];
  readonly sinceMs: number | null;
}

/** The oldest date of `lines`, or null when one of them is undated. */
const sinceOf = (
  doc: MemoryDoc,
  lines: readonly number[],
  dateOf: LineDater
): number | null => {
  let min: number | null = null;
  for (const i of lines) {
    const at = dateOf(doc, i);
    if (at === null) {
      return null;
    }
    min = min === null ? at : Math.min(min, at);
  }
  return min;
};

/**
 * The rule check of one row, or null when no memory line that reaches the
 * row's hits mentions it.
 */
const checkRow = (args: {
  readonly row: CauseRow;
  readonly docs: readonly MemoryDoc[];
  readonly counts: CauseCounts;
  readonly dateOf: LineDater;
  readonly redact: Redactor;
}): RuleCheck | null => {
  const { row, docs, counts, dateOf, redact } = args;
  const probes = probesOf(row);
  if (probes.length === 0) {
    return null;
  }
  const reaches = (doc: MemoryDoc): boolean =>
    total(counts.since(row.key, [{ cover: doc.cover, afterDay: "" }]).hits) > 0;
  // Newest rule first, so the place a row shows is the line whose date it
  // shows. An undated rule sorts last.
  const found: DocMatch[] = sortBy(
    docs.flatMap((doc) => {
      const lines = matchDoc(doc, probes);
      return lines.length > 0 && reaches(doc)
        ? [{ doc, lines, sinceMs: sinceOf(doc, lines, dateOf) }]
        : [];
    }),
    desc((m) => m.sinceMs ?? -1)
  );
  if (found.length === 0) {
    return null;
  }
  const newest = found[0]?.sinceMs ?? null;
  // Every file counts from the newest rule's day: the row says "since <that
  // day>", so an older file must not let earlier hits in.
  const afterDay = newest === null ? "" : dayIso(newest);
  const since = counts.since(
    row.key,
    found.map(({ doc }) => ({ cover: doc.cover, afterDay }))
  );
  // One file read for two agents is listed once: the first doc that reached.
  const listed = new Set<string>();
  const matches: RuleMatch[] = found.flatMap(({ doc, lines }) =>
    lines.flatMap((i) => {
      const id = `${doc.realPath}:${i}`;
      if (listed.has(id)) {
        return [];
      }
      listed.add(id);
      return [
        {
          path: doc.displayPath,
          line: i + 1,
          // Redact the whole line, then clip: a clip first can cut a secret
          // below its pattern's length and ship the cut prefix.
          excerpt: redact.line(doc.lines[i] ?? "", EXCERPT_MAX),
        },
      ];
    })
  );
  return {
    matches: matches.slice(0, MATCH_CAP),
    sinceIso: newest === null ? null : new Date(newest).toISOString(),
    hitsSince: since.hits,
    callsSince: since.calls,
    per1kSince: per1kOf(since.hits, since.calls),
    advice: redact.line(
      adviceOf(row.cause, since.hits, since.calls),
      ADVICE_MAX
    ),
  };
};

/** Attach a rule check to every row a memory file already covers. Pure. */
export const matchRules = (args: {
  readonly rows: readonly CauseRow[];
  readonly docs: readonly MemoryDoc[];
  readonly counts: CauseCounts;
  readonly dateOf: LineDater;
  readonly redact: Redactor;
}): readonly CauseRow[] =>
  args.rows.map((row) => ({
    ...row,
    rule: checkRow({ ...args, row }),
  }));
