/** `peektrace stats` — cross-session friction: what fails, and what costs time.
 *
 * Two ranked tables over every agent's transcripts, both computed in core and
 * printed here without a single number being re-derived. `--json` emits the
 * versioned `peektrace-stats/v1` report unchanged, so an agent can decode it and
 * act on it; a shape change is a `schemaVersion` change.
 *
 * The `FIX` column is advice, not measurement. A fail row prints the `note` core
 * minted for it; a time row prints a fixed line for its job, because a job's
 * remedy is a judgment about a toolchain and has a shorter half-life than the
 * rows it is printed beside.
 */
import { Command, Options } from "@effect/cli";
import { AGENT_IDS, type AgentId } from "@workspace/core";
import type {
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { Console, Effect, Option } from "effect";
import {
  type Globals,
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import { hms, json, percent, shortId, table } from "../render";

/** Chars of a row label or fix a cell keeps. Keys are never clipped: they route. */
const CELL_MAX = 72;

/** Session ids printed beside a row as the way back into the transcripts. */
const REFS_PER_ROW = 3;

const daysOpt = Options.integer("days").pipe(
  Options.withDescription("History window in days (default 60)"),
  Options.optional
);
const agentOpt = Options.text("agent").pipe(
  Options.withDescription(`Agent to scan: ${AGENT_IDS.join("|")}|all`),
  Options.optional
);
const limitOpt = Options.integer("limit").pipe(
  Options.withDescription("Rows kept per table (default 20)"),
  Options.optional
);
const forceOpt = Options.boolean("force").pipe(
  Options.withDescription(
    "Ignore cached shards and re-extract every transcript"
  )
);

/** Per-job advice. Deliberately toolchain-generic: it is read next to any corpus. */
const JOB_FIX: Record<string, string> = {
  "wait/poll":
    "poll a condition instead of sleeping; background the job and check it once",
  test: "scope the run to the changed package or file",
  build: "reuse the build cache; build once per change, not once per check",
  typecheck: "typecheck the package you touched, not the repo",
  lint: "lint the changed files",
  "package-install": "install from the lockfile once per environment",
  net: "cache the response; a repeat fetch inside one run pays twice",
  "script-run":
    "name a recurring one-off as a script so it is cached and reviewed",
  git: "batch status/diff into one call",
  search: "narrow the path; search the package, not the repo",
};

/** Shortest prefix of a session id that still names it. A subagent id is
 * `agent-<hex>`, whose first dash-segment is the word `agent` and names nothing. */
const SID_HEAD_MIN = 8;
const SID_MAX = 14;
const shortSid = (sid: string): string => {
  const head = shortId(sid);
  return head.length >= SID_HEAD_MIN ? head : sid.slice(0, SID_MAX);
};

const clipCell = (s: string): string =>
  s.length > CELL_MAX ? `${s.slice(0, CELL_MAX - 1)}…` : s;

/** The session ids behind a row: the route from a number back to a transcript. */
const refsOf = (samples: readonly { readonly sid: string }[]): string => {
  const ids = [...new Set(samples.map((s) => shortSid(s.sid)))].slice(
    0,
    REFS_PER_ROW
  );
  return ids.length === 0 ? "-" : ids.join(",");
};

/**
 * A share of bash seconds. With no bash seconds in the window there is nothing
 * to take a share of, and the honest cell says so rather than printing 0.0%.
 */
const shareCell = (fraction: number, bashSec: number): string =>
  bashSec > 0 ? percent(fraction) : "not measured";

const failFix = (row: FailRow): string =>
  row.note === null ? "-" : clipCell(row.note);

const timeFix = (row: TimeRow): string => JOB_FIX[row.key] ?? "-";

const failTable = (report: StatsReport, compact: boolean): string =>
  table(
    [
      "SESSIONS",
      "ROWS",
      "PROJ",
      "FLAGGED",
      "WHERE",
      "WHAT",
      "FIX",
      "SEEN IN",
      "DRILL",
    ],
    report.fails.map((row) => [
      String(row.sessions),
      String(row.rows),
      String(row.projects),
      String(row.flagged),
      row.where,
      clipCell(row.label),
      failFix(row),
      refsOf(row.samples),
      `--fail ${row.key}`,
    ]),
    { compact }
  );

const timeTable = (report: StatsReport, compact: boolean): string =>
  table(
    [
      "JOB",
      "RUNS",
      "CO-PRESENT",
      "TOTAL",
      "SHARE",
      "MEDIAN",
      "P95",
      "CAPPED",
      "BG",
      "FIX",
      "SEEN IN",
      "DRILL",
    ],
    report.time.map((row) => [
      row.label,
      String(row.runs),
      String(row.coPresentRuns),
      hms(row.totalSec),
      shareCell(row.share, report.corpus.bashSec),
      hms(row.medianSec),
      hms(row.p95Sec),
      row.cappedRuns === 0 ? "-" : `${row.cappedRuns} / ${hms(row.cappedSec)}`,
      row.bgRuns === 0 ? "-" : `${row.bgRuns} / ${hms(row.bgSec)}`,
      timeFix(row),
      refsOf(row.samples),
      `--time ${row.key}`,
    ]),
    { compact }
  );

/** The waiting panel as one line: the same rows as the `wait/poll` time row. */
const waitLine = (report: StatsReport): string => {
  const w = report.waiting;
  if (w.calls === 0) {
    return "WAITING  no waiting calls in this window";
  }
  const shapes = w.byShape
    .map((s) => `${s.key} ${s.calls}/${hms(s.totalSec)}`)
    .join("  ");
  return [
    `WAITING  ${w.calls} calls  ${hms(w.totalSec)}  ${shareCell(w.share, report.corpus.bashSec)} of shell time  ${w.sessions} sessions  ${w.projects} projects`,
    `         ${shapes}`,
    `         backgrounded ${w.bgRuns} charged ${hms(w.bgSec)}, declaring ${hms(w.bgDeclaredSec)} of sleep (declared is never summed in)`,
  ].join("\n");
};

const header = (report: StatsReport): string => {
  const c = report.corpus;
  return [
    `peektrace stats  ${report.window.days}d window  ${report.window.fromIso} → ${report.window.toIso}`,
    `${c.transcripts} transcripts  ${c.sessions} sessions  ${c.projects} projects  ${c.toolCalls} tool calls  ${c.bashCalls} shell calls  ${hms(c.bashSec)} shell time`,
    `${report.schemaVersion}  detector v${report.detectorVersion}  durations are approximate: they are the call → result gap`,
  ].join("\n");
};

/** Everything that travels with the numbers, plus the legend for an empty FIX. */
const footer = (report: StatsReport): string => {
  const notes = [
    ...report.caveats,
    "FIX '-' means no rule fired for that row: drill it to read the calls",
    "drill a row with: peektrace stats drill <DRILL>",
  ];
  const skipped =
    report.skipped.length === 0
      ? []
      : [
          `${report.skipped.length} files could not be read and are not counted`,
        ];
  return [
    "NOTES",
    ...notes.map((n) => `- ${n}`),
    ...skipped.map((n) => `- ${n}`),
  ].join("\n");
};

const renderReport = (report: StatsReport, compact: boolean): string =>
  [
    header(report),
    "",
    "WHAT FAILS  (ranked by sessions spanned, then rows)",
    failTable(report, compact),
    "",
    "WHAT COSTS TIME  (ranked by seconds; each command is charged to exactly one job)",
    timeTable(report, compact),
    "",
    waitLine(report),
    "",
    footer(report),
  ].join("\n");

/** Resolve `--agent`: absent or `all` scans every agent that has a parser. */
const scopeOf = (agent: Option.Option<string>) => {
  const value = Option.getOrElse(agent, () => "all");
  if (value === "all") {
    return Effect.succeed<{ agents?: readonly AgentId[] }>({});
  }
  const found = AGENT_IDS.find((id) => id === value);
  return found === undefined
    ? Effect.fail(
        new CliUserError({
          message: `Unknown agent "${value}". Known: ${AGENT_IDS.join(", ")}, all.`,
        })
      )
    : Effect.succeed({ agents: [found] as readonly AgentId[] });
};

/** The window/limit half of a request, with absent flags left to core's defaults. */
const windowOf = (
  days: Option.Option<number>,
  limit: Option.Option<number>
) => ({
  ...(Option.isSome(days) ? { days: days.value } : {}),
  ...(Option.isSome(limit) ? { limit: limit.value } : {}),
});

/** `stats drill` — the calls behind one aggregate key. */
const makeStatsDrill = (globals: GlobalsAccessor) =>
  Command.make(
    "drill",
    {
      fail: Options.text("fail").pipe(
        Options.withDescription("Fail-table key to expand"),
        Options.optional
      ),
      time: Options.text("time").pipe(
        Options.withDescription("Time-table job to expand"),
        Options.optional
      ),
      cluster: Options.text("cluster").pipe(
        Options.withDescription("Error-cluster signature to expand"),
        Options.optional
      ),
      wait: Options.text("wait").pipe(
        Options.withDescription("Waiting-panel key to expand"),
        Options.optional
      ),
      days: daysOpt,
      agent: agentOpt,
      limit: limitOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    (opts) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: opts.json, readOnly: opts.readOnly });
        const picked = (
          [
            ["fail", opts.fail],
            ["time", opts.time],
            ["cluster", opts.cluster],
            ["wait", opts.wait],
          ] as const
        ).flatMap(([table_, key]) =>
          Option.isSome(key) ? [{ table: table_, key: key.value }] : []
        );
        const target = picked.length === 1 ? picked[0] : undefined;
        if (target === undefined) {
          return yield* new CliUserError({
            message:
              "Pass exactly one of --fail, --time, --cluster or --wait with a key from the table.",
          });
        }
        const scope = yield* scopeOf(opts.agent);
        const result = yield* withClient(g, (client) =>
          client.stats.drill({
            ...scope,
            ...windowOf(opts.days, opts.limit),
            key: target.key,
            table: target.table,
          })
        );
        if (g.json) {
          return yield* Console.log(json(result));
        }
        const rows = result.hits.map((hit) => [
          hit.ts ?? "-",
          shortSid(hit.sid),
          hit.project,
          hit.tool,
          hit.durSec === null ? "-" : hms(hit.durSec),
          hit.failed ? "fail" : "-",
          clipCell(hit.line),
        ]);
        const shown = `${result.table} ${result.key}: ${result.hits.length} of ${result.total} calls`;
        return yield* Console.log(
          `${shown}\n${table(
            ["TS", "SESSION", "PROJECT", "TOOL", "DUR", "STATE", "LINE"],
            rows,
            { compact: g.compact }
          )}`
        );
      })
  );

/** `stats refresh` — re-extract the fact cache without computing a report. */
const makeStatsRefresh = (globals: GlobalsAccessor) =>
  Command.make(
    "refresh",
    {
      agent: agentOpt,
      force: forceOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    (opts) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: opts.json, readOnly: opts.readOnly });
        const scope = yield* scopeOf(opts.agent);
        const summary = yield* withClient(g, (client) =>
          client.stats.refresh({ ...scope, force: opts.force })
        );
        if (g.json) {
          return yield* Console.log(json(summary));
        }
        return yield* Console.log(
          table(
            ["FIELD", "VALUE"],
            [
              ["transcripts", String(summary.transcripts)],
              ["extracted", String(summary.extracted)],
              ["from cache", String(summary.cached)],
              ["facts", String(summary.facts)],
              ["skipped", String(summary.skipped.length)],
            ],
            { compact: g.compact }
          )
        );
      })
  );

/** Run the report and print it in the caller's chosen form. */
const runReport = (args: {
  readonly days: Option.Option<number>;
  readonly g: Globals;
  readonly limit: Option.Option<number>;
  readonly scope: { readonly agents?: readonly AgentId[] };
}) =>
  Effect.gen(function* () {
    const { days, g, limit, scope } = args;
    const result = yield* withClient(g, (client) =>
      client.stats.report({ ...scope, ...windowOf(days, limit) })
    );
    return yield* Console.log(
      g.json ? json(result) : renderReport(result, g.compact)
    );
  });

/** `stats` — the two ranked tables, plus `drill` and `refresh`. */
export const makeStats = (globals: GlobalsAccessor) =>
  Command.make(
    "stats",
    {
      days: daysOpt,
      agent: agentOpt,
      limit: limitOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    (opts) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: opts.json, readOnly: opts.readOnly });
        const scope = yield* scopeOf(opts.agent);
        return yield* runReport({
          days: opts.days,
          g,
          limit: opts.limit,
          scope,
        });
      })
  ).pipe(
    Command.withSubcommands([
      makeStatsDrill(globals),
      makeStatsRefresh(globals),
    ])
  );
