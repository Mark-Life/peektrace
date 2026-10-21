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
  type FindingFormat,
  ruleLine,
  ruleState,
  timeNote,
} from "./findings";
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

/** The flags every suggested command repeats, so it reads the same window. */
const flagsOf = (report: StatsReport, agent: AgentId | null): string =>
  `--days ${report.window.days}${agent === null ? "" : ` --agent ${agent}`}`;

const windowLine = (report: StatsReport, agent: AgentId | null): string =>
  `last ${report.window.days} days, ${agent ?? "all agents"}`;

/** `zsh-glob-flag` from `cause:zsh-glob-flag`: the id `--cause` accepts. */
const causeId = (row: CauseRow): string => row.key.replace(CAUSE_PREFIX, "");

/** `main 1 / sub 9`. */
const split = (
  s: { readonly main: number; readonly nested: number },
  f: FindingFormat
): string => `main ${f.count(s.main)} / sub ${f.count(s.nested)}`;

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
      return [line, `  Too new to measure; ${row.rule.advice}.`];
    default:
      return [line];
  }
};

const causePrompt = (args: {
  readonly agent: AgentId | null;
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly row: CauseRow;
}): readonly string[] => {
  const { agent, f, report, row } = args;
  const exit0 = row.hits - row.flagged;
  const listed = (items: readonly string[]) => items.join(" · ");
  return [
    `Pattern: ${row.label}`,
    `- cause: ${causeId(row)} · fix lives in: ${KIND_WHERE[row.kind]}`,
    `- hits: ${f.count(row.hits)}${exit0 > 0 ? ` (${f.count(exit0)} exited 0)` : ""} in ${f.count(row.sessions)} sessions, ${f.count(row.projects)} projects · ${split(row.hitsBy, f)}`,
    ...(row.pairs > 0
      ? [
          `- wasted: ${causeImpact(row, f)} in ${f.count(row.retrySessions)} sessions`,
        ]
      : []),
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
    `All calls: peektrace stats drill --cause ${row.key} ${flagsOf(report, agent)} --json`,
  ];
};

const failPrompt = (args: {
  readonly agent: AgentId | null;
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly row: FailRow;
}): readonly string[] => {
  const { agent, f, report, row } = args;
  return [
    `Pattern: ${row.label} (${row.where})`,
    `- key: ${row.key}${row.detector === null ? "" : ` · detector: ${row.detector}`}`,
    `- calls: ${f.count(row.rows)} (${f.count(row.flagged)} set the error flag) in ${f.count(row.sessions)} sessions, ${f.count(row.projects)} projects`,
    ...(row.note === null ? [] : [`- note: ${row.note}`]),
    ...samplesBlock({ pairs: [], paths: report.paths, samples: row.samples }),
    `All calls: peektrace stats drill --fail ${row.key} ${flagsOf(report, agent)} --json`,
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
  readonly agent: AgentId | null;
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly row: TimeRow;
}): readonly string[] => {
  const { agent, f, report, row } = args;
  const note = timeNote(row, f);
  const share =
    report.corpus.bashSec > 0 ? ` (${f.pct(row.share)} of shell time)` : "";
  return [
    `Pattern: shell time charged to ${row.label}`,
    `- job: ${row.key} · ${f.count(row.runs)} runs, ${f.hms(row.totalSec)}${share} · median ${f.hms(row.medianSec)}, p95 ${f.hms(row.p95Sec)}`,
    ...(row.key === "wait/poll" ? waitLines(report, f) : []),
    ...(note === null ? [] : [`- note: ${note}`]),
    ...samplesBlock({ pairs: [], paths: report.paths, samples: row.samples }),
    `All calls: peektrace stats drill --time ${row.key} ${flagsOf(report, agent)} --json`,
  ];
};

/** One row's prompt body, or null when the key names no row. */
const rowBody = (args: {
  readonly agent: AgentId | null;
  readonly f: FindingFormat;
  readonly report: StatsReport;
  readonly target: Exclude<PromptTarget, { table: "report" }>;
}): readonly string[] | null => {
  const { agent, f, report, target } = args;
  switch (target.table) {
    case "cause": {
      const row = report.causes.find(
        (r) => r.key === target.key || causeId(r) === target.key
      );
      return row === undefined ? null : causePrompt({ agent, f, report, row });
    }
    case "fail": {
      const row = report.fails.find((r) => r.key === target.key);
      return row === undefined ? null : failPrompt({ agent, f, report, row });
    }
    default: {
      const row = report.time.find((r) => r.key === target.key);
      return row === undefined ? null : timePrompt({ agent, f, report, row });
    }
  }
};

/** One cause in the report prompt: its hits and what its retries cost, with
 * the units named once in the line above the list. */
const causeBrief = (row: CauseRow, f: FindingFormat): string => {
  const hits = `${f.count(row.hits)} hits`;
  if (row.pairs === 0) {
    return `${hits} · ${f.count(row.sessions)} sessions`;
  }
  const what = row.failing ? "retries" : "read-backs";
  return [
    hits,
    `${f.count(row.pairs)} ${what} (${split(row.pairsBy, f)})`,
    ...(row.secMeasured > 0 ? [f.hms(row.retrySec)] : []),
    ...(row.tokensMeasured > 0 ? [`${f.tokens(row.retryCtxTokens)} ctx`] : []),
  ].join(" · ");
};

/** The data block of the report prompt: corpus, then each table's top rows. */
const reportBody = (args: {
  readonly agent: AgentId | null;
  readonly f: FindingFormat;
  readonly report: StatsReport;
}): readonly string[] => {
  const { agent, f, report } = args;
  const c = report.corpus;
  const main = c.kinds.main ?? 0;
  const causes = report.causes.filter((r) => r.hits > 0);
  const pairs = causes.reduce((n, r) => n + r.pairs, 0);
  const sec = causes.reduce((n, r) => n + r.retrySec, 0);
  const ctx = causes.reduce((n, r) => n + r.retryCtxTokens, 0);
  const firing = causes.flatMap((r) =>
    r.rule !== null && ruleState(r.rule) === "firing"
      ? [`${causeId(r)} (${ruleLine(r.rule)})`]
      : []
  );
  const flags = flagsOf(report, agent);
  return [
    `Data (${report.window.fromIso.slice(0, 10)} → ${report.window.toIso.slice(0, 10)}):`,
    `- corpus: ${f.count(c.transcripts)} transcripts (${f.count(main)} main, ${f.count(c.transcripts - main)} subagent), ${f.count(c.projects)} projects, ${f.count(c.toolCalls)} tool calls, ${f.hms(c.bashSec)} shell time`,
    `- avoidable retries: ${f.count(pairs)} round trips, ${f.hms(sec)}, ${f.tokens(ctx)} ctx tokens re-read`,
    "- causes (--cause id; ctx = context tokens the retries re-read):",
    ...causes
      .slice(0, TOP_CAUSES)
      .map(
        (r) =>
          `  - ${causeId(r)}: ${causeBrief(r, f)}${r.rule === null ? "" : " · rule in memory"}`
      ),
    "- failures (--fail key):",
    ...report.fails
      .slice(0, TOP_FAILS)
      .map(
        (r) =>
          `  - ${r.key} (${r.label}): ${f.count(r.rows)} calls, ${f.count(r.sessions)} sessions${r.note === null ? "" : ` · ${r.note}`}`
      ),
    `- shell time (--time job): ${report.time
      .slice(0, TOP_TIME)
      .map((r) => `${r.key} ${f.hms(r.totalSec)} (${f.pct(r.share)})`)
      .join(" · ")}`,
    ...(report.waiting.calls > 0
      ? [
          `- waiting: ${f.count(report.waiting.calls)} calls, ${f.hms(report.waiting.totalSec)}, ${f.count(report.waiting.bgRuns)} backgrounded`,
        ]
      : []),
    `- rules in memory but still firing: ${firing.length === 0 ? "none" : firing.join("; ")}`,
    "More data:",
    `- full report: peektrace stats ${flags} --json`,
    `- one row, with transcript paths: peektrace stats prompt --cause <id> | --fail <key> | --time <job> ${flags}`,
    `- every call of a row: peektrace stats drill --cause cause:<id> | --fail <key> | --time <job> ${flags} --json`,
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
  readonly report: StatsReport;
  readonly target: PromptTarget;
}): string | null => {
  const { format: f, report, target } = args;
  const agent = args.agent ?? null;
  const scope = windowLine(report, agent);
  if (target.table === "report") {
    return [
      GOAL,
      `Below is measured data from peektrace (${scope}). Pick the patterns worth fixing, pull their calls with the commands at the end, and read the transcripts to find root causes. ${ASK}`,
      "",
      ...reportBody({ agent, f, report }),
    ].join("\n");
  }
  const body = rowBody({ agent, f, report, target });
  return body === null
    ? null
    : [
        GOAL,
        `Below is measured data for one pattern from peektrace (${scope}). Read the sample transcripts to confirm it and find its root cause. ${ASK}`,
        "",
        ...body,
      ].join("\n");
};
