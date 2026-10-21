/** Investigation prompts: the report, or one row of it, as text an agent acts on.
 *
 * Stats exist so the user can cut round trips, compute, waiting and failed
 * calls. A prompt hands that goal to an agent with the measured data and the
 * routes back to the calls: the transcript path and tool-use id of each sample,
 * and the CLI commands that pull more. Agents are capable readers, so the text
 * is a short goal and a tight key-value block, nothing else.
 *
 * Every surface (CLI, inspector) calls `statsPrompt` with its own number
 * spelling, so the wording cannot drift between them. The data comes from the
 * report, already redacted in core.
 */
import type { AgentId } from "../agent-id";
import {
  causeImpact,
  causeSpread,
  type FindingFormat,
  ruleLine,
  ruleState,
  timeNote,
  timeSpread,
} from "./findings";
import { DEFAULT_DRILL_PAGE } from "./rank";
import type {
  CauseKind,
  CauseRow,
  FailRow,
  SampleRef,
  StatsReport,
  TimeRow,
} from "./schema";

/** What a prompt is about: the whole report, or one row of one table. */
export type PromptTarget =
  | { readonly table: "report" }
  | { readonly table: "cause" | "fail" | "time"; readonly key: string };

/** Rows of each table the report prompt lists. */
const TOP_CAUSES = 6;
const TOP_FAILS = 4;
const TOP_TIME = 4;
/** Sample calls a row prompt names; the drill command lists the rest. */
const SAMPLES_SHOWN = 3;

const GOAL =
  "Goal: make my coding agents faster and cheaper: fewer model round trips, less compute and waiting, fewer failed tool calls.";

const ASK =
  "Propose concrete changes to my environment (shell config, installed tools, hooks), AGENTS.md/CLAUDE.md, skills or prompts that prevent it. Rank by impact; show exact edits; don't apply until I confirm.";

/** Where a cause's fix lives, in words an agent reads without the schema. */
const KIND_WHERE: Record<CauseKind, string> = {
  rule: "memory file (AGENTS.md/CLAUDE.md)",
  env: "host environment",
  workflow: "workflow or subagent prompt",
  none: "unknown (unclassified)",
};

const CAUSE_PREFIX = /^cause:/;
const MS_SUFFIX = /\.\d+Z$/;
/** Core's advice opens with the condition a firing rule already meets. */
const KEEPS_FIRING = /^if it keeps firing:\s*/;

/** Where the report came from: the agent it was scoped to, and the
 * `peektrace serve` URL when it was read remotely. */
interface Scope {
  readonly agent: AgentId | null;
  readonly remote: string | null;
}

/** `peektrace stats <sub> --days 30`: a command that reads the same corpus and
 * window the report did. */
const commandOf = (report: StatsReport, scope: Scope, sub: string): string =>
  [
    "peektrace",
    ...(scope.remote === null ? [] : [`--remote ${scope.remote}`]),
    "stats",
    ...(sub === "" ? [] : [sub]),
    `--days ${report.window.days}`,
    ...(scope.agent === null ? [] : [`--agent ${scope.agent}`]),
  ].join(" ");

/** The drill command for one row, with what its page holds. */
const drillLine = (args: {
  readonly flag: string;
  readonly key: string;
  readonly report: StatsReport;
  readonly scope: Scope;
}): string =>
  `Calls (first ${DEFAULT_DRILL_PAGE}, .total counts all, .paths has transcripts): ${commandOf(args.report, args.scope, "drill")} --${args.flag} ${args.key} --json`;

const windowLine = (report: StatsReport, scope: Scope): string =>
  [
    `last ${report.window.days} days, ${scope.agent ?? "all agents"}`,
    ...(scope.remote === null
      ? []
      : [`read from ${scope.remote}: transcript paths are on that host`]),
  ].join("; ");

/** `zsh-glob-flag` from `cause:zsh-glob-flag`: the id `--cause` accepts. */
const causeId = (row: CauseRow): string => row.key.replace(CAUSE_PREFIX, "");

/** One sample: when, which call, which transcript, then the evidence. */
const sampleLines = (args: {
  readonly paths: StatsReport["paths"];
  readonly retry: SampleRef | null;
  readonly sample: SampleRef;
}): readonly string[] => {
  const { paths, retry, sample } = args;
  const head = [
    sample.ts?.replace(MS_SUFFIX, "Z") ?? null,
    sample.toolUseId,
    paths[sample.sid] ?? `session ${sample.sid}`,
  ].filter((part) => part !== null);
  return [
    `- ${head.join(" · ")}`,
    retry === null
      ? `  call: ${sample.line}`
      : `  failed: ${sample.line}\n  retry: ${retry.line}`,
  ];
};

/** A row's samples, retry pairs first, each call once. */
const samplesBlock = (args: {
  readonly pairs: CauseRow["pairSamples"];
  readonly paths: StatsReport["paths"];
  readonly samples: readonly SampleRef[];
}): readonly string[] => {
  const { pairs, paths, samples } = args;
  const idOf = (s: SampleRef) => `${s.sid}#${s.seq}`;
  const paired = new Set(pairs.map((p) => idOf(p.failed)));
  const lines = [
    ...pairs.map((p) => ({ retry: p.retry, sample: p.failed })),
    ...samples
      .filter((s) => !paired.has(idOf(s)))
      .map((sample) => ({ retry: null, sample })),
  ]
    .slice(0, SAMPLES_SHOWN)
    .flatMap(({ retry, sample }) => sampleLines({ paths, retry, sample }));
  return lines.length === 0
    ? []
    : [pairs.length > 0 ? "Samples (failed → retry):" : "Samples:", ...lines];
};

const capitalized = (text: string): string =>
  `${text.charAt(0).toUpperCase()}${text.slice(1)}`;

/** What the rule check asks of the agent, beyond the line itself. */
const ruleAsk = (row: CauseRow): readonly string[] => {
  if (row.rule === null) {
    return row.kind === "rule" ? ["- rule: none in AGENTS.md/CLAUDE.md"] : [];
  }
  const line = `- ${ruleLine(row.rule)}`;
  switch (ruleState(row.rule)) {
    case "firing":
      return [
        line,
        `  The rule is in memory yet still fires: find why it isn't followed and fix it in a way that doesn't depend on the agent remembering (e.g. ${row.rule.advice.replace(KEEPS_FIRING, "")}).`,
      ];
    case "unmeasured":
      return [line, `  ${capitalized(row.rule.advice)}.`];
    default:
      return [line];
  }
};

const causePrompt = (args: {
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly row: CauseRow;
  readonly scope: Scope;
}): readonly string[] => {
  const { f, report, row, scope } = args;
  const listed = (items: readonly string[]) => items.join(" · ");
  // A rule already in memory leaves the fix's home open: that is the question.
  const where =
    row.rule === null ? ` · fix lives in: ${KIND_WHERE[row.kind]}` : "";
  return [
    `Pattern: ${row.label}`,
    `- cause: ${causeId(row)}${where}`,
    `- ${f.count(row.sessions)} sessions · ${causeSpread(row, f)}`,
    ...(row.pairs > 0 ? [`- wasted: ${causeImpact(row, f)}`] : []),
    ...(row.details.length > 0
      ? [
          `- culprits: ${listed(row.details.map((d) => `${d.value} ×${f.count(d.hits)}`))}`,
        ]
      : []),
    ...(row.diffs.length > 0
      ? [
          `- retries changed: ${listed(row.diffs.map((d) => `${d.from} → ${d.to} ×${f.count(d.pairs)}`))}`,
        ]
      : []),
    ...(row.fix === null ? [] : [`- suggested fix: ${row.fix}`]),
    ...ruleAsk(row),
    ...samplesBlock({
      pairs: row.pairSamples,
      paths: report.paths,
      samples: row.samples,
    }),
    drillLine({ flag: "cause", key: causeId(row), report, scope }),
  ];
};

const failPrompt = (args: {
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly row: FailRow;
  readonly scope: Scope;
}): readonly string[] => {
  const { f, report, row, scope } = args;
  return [
    `Pattern: ${row.label} (${row.where})`,
    `- key: ${row.key}${row.detector === null ? "" : ` · detector: ${row.detector}`}`,
    `- calls: ${f.count(row.rows)} (${f.count(row.flagged)} set the error flag) in ${f.count(row.sessions)} sessions, ${f.count(row.projects)} projects`,
    ...(row.note === null ? [] : [`- note: ${row.note}`]),
    ...samplesBlock({ pairs: [], paths: report.paths, samples: row.samples }),
    drillLine({ flag: "fail", key: row.key, report, scope }),
  ];
};

/** The waiting panel's shapes, for the `wait/poll` row they describe. */
const waitLines = (
  report: StatsReport,
  f: FindingFormat
): readonly string[] => {
  const w = report.waiting;
  const shapes = w.byShape
    .filter((s) => s.calls > 0)
    .map((s) => `${s.key} ${f.count(s.calls)} calls ${f.hms(s.totalSec)}`)
    .join(" · ");
  return w.calls === 0
    ? []
    : [
        `- shapes: ${shapes}; ${f.count(w.bgRuns)} backgrounded, declaring ${f.hms(w.bgDeclaredSec)} of sleep`,
      ];
};

const timePrompt = (args: {
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly row: TimeRow;
  readonly scope: Scope;
}): readonly string[] => {
  const { f, report, row, scope } = args;
  const note = timeNote(row, f);
  return [
    `Pattern: shell time charged to ${row.label}`,
    `- job: ${row.key} · ${f.hms(row.totalSec)} · ${timeSpread(row, report.corpus.bashSec, f)} · median ${f.hms(row.medianSec)}, p95 ${f.hms(row.p95Sec)}`,
    ...(row.key === "wait/poll" ? waitLines(report, f) : []),
    ...(note === null ? [] : [`- note: ${note}`]),
    ...samplesBlock({ pairs: [], paths: report.paths, samples: row.samples }),
    drillLine({ flag: "time", key: row.key, report, scope }),
  ];
};

/** One row's prompt body, or null when the key names no row. */
const rowBody = (args: {
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly scope: Scope;
  readonly target: Exclude<PromptTarget, { table: "report" }>;
}): readonly string[] | null => {
  const { f, report, scope, target } = args;
  switch (target.table) {
    case "cause": {
      const row = report.causes.find(
        (r) => r.key === target.key || causeId(r) === target.key
      );
      return row === undefined ? null : causePrompt({ f, report, row, scope });
    }
    case "fail": {
      const row = report.fails.find((r) => r.key === target.key);
      return row === undefined ? null : failPrompt({ f, report, row, scope });
    }
    default: {
      const row = report.time.find((r) => r.key === target.key);
      return row === undefined ? null : timePrompt({ f, report, row, scope });
    }
  }
};

/** A header line and its rows, or nothing when the table has no rows. */
const section = (header: string, rows: readonly string[]): readonly string[] =>
  rows.length === 0 ? [] : [header, ...rows];

/** Which causes' memory rules still fire, and which are not measured yet. A
 * rule nobody measured is not a rule that works, so it never reads `none`. */
const ruleLines = (causes: readonly CauseRow[]): readonly string[] => {
  const ids = (state: "firing" | "quiet" | "unmeasured") =>
    causes.flatMap((r) =>
      r.rule !== null && ruleState(r.rule) === state
        ? [
            state === "firing"
              ? `${causeId(r)} (${ruleLine(r.rule)})`
              : causeId(r),
          ]
        : []
    );
  const firing = ids("firing");
  const quiet = ids("quiet");
  const unmeasured = ids("unmeasured");
  return [
    ...(firing.length > 0 || quiet.length > 0
      ? [
          `- rules in memory but still firing: ${firing.length === 0 ? "none" : firing.join("; ")}`,
        ]
      : []),
    ...(unmeasured.length > 0
      ? [
          `- rules in memory, still-firing rate not measured yet: ${unmeasured.join(", ")}`,
        ]
      : []),
  ];
};

/** The data block of the report prompt: corpus, then each table's top rows. */
const reportBody = (args: {
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly scope: Scope;
}): readonly string[] => {
  const { f, report, scope } = args;
  const c = report.corpus;
  const main = c.kinds.main ?? 0;
  const causes = report.causes.filter((r) => r.hits > 0);
  const pairs = causes.reduce((n, r) => n + r.pairs, 0);
  const sec = causes.reduce((n, r) => n + r.retrySec, 0);
  const ctx = causes.reduce((n, r) => n + r.retryCtxTokens, 0);
  const time = report.time
    .slice(0, TOP_TIME)
    .map((r) => `${r.key} ${f.hms(r.totalSec)} (${f.pct(r.share)})`);
  return [
    `Data (${report.window.fromIso.slice(0, 10)} → ${report.window.toIso.slice(0, 10)}):`,
    `- corpus: ${f.count(c.transcripts)} transcripts (${f.count(main)} main, ${f.count(c.transcripts - main)} subagent), ${f.count(c.projects)} projects, ${f.count(c.toolCalls)} tool calls, ${f.hms(c.bashSec)} shell time`,
    ...(pairs > 0
      ? [
          `- avoidable retries: ${f.count(pairs)} round trips, ${f.hms(sec)}, ${f.tokens(ctx)} ctx tokens re-read`,
        ]
      : []),
    ...section(
      "- causes (--cause id):",
      causes
        .slice(0, TOP_CAUSES)
        .map(
          (r) =>
            `  - ${causeId(r)}: ${f.count(r.hits)} hits · ${causeImpact(r, f)}${r.rule === null ? "" : " · rule in memory"}`
        )
    ),
    ...section(
      "- failures (--fail key):",
      report.fails
        .slice(0, TOP_FAILS)
        .map(
          (r) =>
            `  - ${r.key} (${r.label}): ${f.count(r.rows)} calls, ${f.count(r.sessions)} sessions${r.note === null ? "" : ` · ${r.note}`}`
        )
    ),
    ...(time.length > 0
      ? [`- shell time (--time job): ${time.join(" · ")}`]
      : []),
    ...(report.waiting.calls > 0
      ? [
          `- waiting: ${f.count(report.waiting.calls)} calls, ${f.hms(report.waiting.totalSec)}, ${f.count(report.waiting.bgRuns)} backgrounded`,
        ]
      : []),
    ...ruleLines(causes),
    "More data (--cause, --fail or --time picks the row):",
    `- full report: ${commandOf(report, scope, "")} --json`,
    `- one row, with transcript paths: ${commandOf(report, scope, "prompt")} --cause <id>`,
    `- a row's calls: ${commandOf(report, scope, "drill")} --cause <id> --json`,
  ];
};

/**
 * The investigation prompt for the whole report or one of its rows, or null
 * when the target names no row. Pure: the same report and format always give
 * the same text, on every surface.
 */
export const statsPrompt = (args: {
  /** The agent the report was scoped to, or null for every agent. */
  readonly agent?: AgentId | null;
  readonly format: FindingFormat;
  /** The `peektrace serve` URL the report was read from, or null when local. */
  readonly remote?: string | null;
  readonly report: StatsReport;
  readonly target: PromptTarget;
}): string | null => {
  const { format: f, report, target } = args;
  const scope: Scope = {
    agent: args.agent ?? null,
    remote: args.remote ?? null,
  };
  const where = windowLine(report, scope);
  if (target.table === "report") {
    return [
      GOAL,
      `Below is measured data from peektrace (${where}). Pick the patterns worth fixing, pull their calls with the commands at the end, and read the transcripts to find root causes. ${ASK}`,
      "",
      ...reportBody({ f, report, scope }),
    ].join("\n");
  }
  const body = rowBody({ f, report, scope, target });
  return body === null
    ? null
    : [
        GOAL,
        `Below is measured data for one pattern from peektrace (${where}). Read the sample transcripts to confirm it and find its root cause. ${ASK}`,
        "",
        ...body,
      ].join("\n");
};
