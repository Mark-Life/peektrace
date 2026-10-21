/** `peektrace stats` — cross-session friction: what to fix first, which named
 * causes cost retries, what fails, and what costs time.
 *
 * Ranked tables over every agent's transcripts, all computed in core and
 * printed here without a single number being re-derived. `--json` emits the
 * versioned `peektrace-stats/v2` report unchanged, so an agent can decode it and
 * act on it; a shape change is a `schemaVersion` change.
 *
 * The `FIX` column is advice, not measurement. A fail row prints the `note` core
 * minted for it; a time row prints a fixed line for its job, because a job's
 * remedy is a judgment about a toolchain and has a shorter half-life than the
 * rows it is printed beside.
 */
import { Command, Options } from "@effect/cli";
import { AGENT_IDS, type AgentId } from "@workspace/core";
import { RETRY_WINDOW } from "@workspace/core/services/stats/detectors/retry-gates";
import {
  type FindingFormat,
  findingsOf,
  NO_RULE,
  noSuggestionReason,
  rateText,
  ruleLine,
  suggestedRules,
} from "@workspace/core/services/stats/findings";
import type {
  CauseRow,
  FailRow,
  SampleRef,
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
import { hms, json, percent, shortId, table, tokens } from "../render";

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

/** Where a sample sits: its own session, or a subagent under `parent`. */
type SessionRef = Pick<SampleRef, "parent" | "sid">;

/** A drill hit's session. A subagent prints `<parent>/<subagent>`, the two
 * short ids `sessions analyze --parent <parent> <subagent>` takes. */
const hitRef = ({ parent, sid }: SessionRef): string =>
  parent === null ? shortSid(sid) : `${shortSid(parent)}/${shortSid(sid)}`;

/** The session ids behind a row: the route from a number back to a transcript. */
const refsOf = (samples: readonly SessionRef[]): string => {
  // A subagent id is not a listed session; name the parent it ran under.
  const ids = [
    ...new Set(
      samples.map((s) =>
        s.parent === null ? shortSid(s.sid) : `${shortSid(s.parent)}/sub`
      )
    ),
  ].slice(0, REFS_PER_ROW);
  return ids.length === 0 ? "-" : ids.join(",");
};

/**
 * A share of bash seconds. With no bash seconds in the window there is nothing
 * to take a share of, and the honest cell says so rather than printing 0.0%.
 */
const shareCell = (fraction: number, bashSec: number): string =>
  bashSec > 0 ? percent(fraction) : "not measured";

/** A fail row's fix: core's note, or the explicit "no rule to suggest". */
const failFix = (row: FailRow): string =>
  row.note === null ? NO_RULE : clipCell(row.note);

/** Findings printed above the tables; the rest are in the tables below. */
const FINDINGS_SHOWN = 5;

const FORMAT: FindingFormat = {
  count: (n) => n.toLocaleString("en-US"),
  hms,
  pct: percent,
  tokens,
};

/** The cold-start list: one change per line, with what it cost. */
const findingsBlock = (report: StatsReport): string => {
  const found = findingsOf(report, FORMAT);
  if (found.length === 0) {
    return "FIX FIRST  no row names a change in this window";
  }
  const lines = found
    .slice(0, FINDINGS_SHOWN)
    .flatMap((f, i) => [
      `${i + 1}. ${f.title}  [${f.impact}]`,
      `   ${f.note}`,
      `   ${f.spread}`,
    ]);
  const more =
    found.length > FINDINGS_SHOWN
      ? [`   … ${found.length - FINDINGS_SHOWN} more in the tables below`]
      : [];
  return [
    "FIX FIRST  (named causes lead; never a blended score)",
    ...lines,
    ...more,
  ].join("\n");
};

const CAUSE_PREFIX = /^cause:/;

/** The cause a row names, without the routing prefix. */
const causeName = (row: CauseRow): string => row.key.replace(CAUSE_PREFIX, "");

/** Core's one-line rule check, or `-` when no memory line states the rule. */
const ruleCell = (row: CauseRow): string =>
  row.rule === null ? "-" : ruleLine(row.rule);

const causeTable = (report: StatsReport, compact: boolean): string =>
  table(
    [
      "CAUSE",
      "KIND",
      "HITS",
      "PAIRS",
      "SESS",
      "PROJ",
      "HITS M/S",
      "PER 1K IN WINDOW M/S",
      "RETRY SEC",
      "CTX TOK",
      "RULE",
      "FIX",
      "SEEN IN",
      "DRILL",
    ],
    report.causes.map((row) => [
      causeName(row),
      row.kind,
      String(row.hits),
      String(row.pairs),
      String(row.retrySessions),
      String(row.projects),
      `${row.hitsBy.main}/${row.hitsBy.nested}`,
      `${rateText(row.per1k.main, row.hitsBy.main)}/${rateText(row.per1k.nested, row.hitsBy.nested)}`,
      row.secMeasured === 0 ? "-" : hms(row.retrySec),
      row.tokensMeasured === 0 ? "-" : tokens(row.retryCtxTokens),
      ruleCell(row),
      clipCell(row.fix ?? NO_RULE),
      refsOf(row.samples),
      `--cause ${row.key}`,
    ]),
    { compact }
  );

const suggestedBlock = (report: StatsReport): string => {
  const lines = suggestedRules(report).map((line) => `- ${line}`);
  const none = noSuggestionReason(report);
  return none === null
    ? [
        "SUGGESTED RULES  (copy into CLAUDE.md / AGENTS.md; causes already stated are not repeated)",
        ...lines,
      ].join("\n")
    : `SUGGESTED RULES  none: ${none}`;
};

const timeFix = (row: TimeRow): string => JOB_FIX[row.key] ?? NO_RULE;

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
    `FIX '${NO_RULE}' means no rule fired for that row: drill it to read the calls`,
    `PAIRS are failed calls followed within ${RETRY_WINDOW} calls by a similar succeeding call; SESS counts sessions with a pair`,
    "HITS M/S splits hits between main sessions and subagents; PER 1K IN WINDOW M/S is hits per 1,000 calls that could have hit, per side, over the whole window (a RULE rate counts only since its rule line appeared)",
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

/** The whole text report: findings first, then the tables, then the notes. */
export const renderReport = (report: StatsReport, compact: boolean): string =>
  [
    header(report),
    "",
    findingsBlock(report),
    "",
    "AVOIDABLE RETRIES  (named causes, ranked by sessions with a retry, then sessions, then pairs)",
    causeTable(report, compact),
    "",
    suggestedBlock(report),
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
      cause: Options.text("cause").pipe(
        Options.withDescription(
          "Cause-table key to expand into failed → retry pairs"
        ),
        Options.optional
      ),
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
            ["cause", opts.cause],
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
              "Pass exactly one of --cause, --fail, --time, --cluster or --wait with a key from the table.",
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
        // Only a cause drill pairs its calls with their retries.
        const paired = result.table === "cause";
        const rows = result.hits.map((hit) => [
          hit.ts ?? "-",
          hitRef(hit),
          hit.project,
          hit.tool,
          hit.durSec === null ? "-" : hms(hit.durSec),
          hit.failed ? "fail" : "-",
          clipCell(hit.line),
          ...(paired
            ? [hit.pairLine === null ? "-" : clipCell(hit.pairLine)]
            : []),
        ]);
        const shown = `${result.table} ${result.key}: ${result.hits.length} of ${result.total} calls`;
        return yield* Console.log(
          `${shown}\n${table(
            [
              "TS",
              "SESSION",
              "PROJECT",
              "TOOL",
              "DUR",
              "STATE",
              "LINE",
              ...(paired ? ["RETRY"] : []),
            ],
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
