/** `peektrace stats usage` — tokens and estimated cost per day, month or
 * 5-hour block. Core computes every number; this file only prints them. */
import { Command, Options } from "@effect/cli";
import type {
  UsageReport,
  UsageRow,
  UsageTotals,
} from "@workspace/core/services/stats/usage-schema";
import { Console, Effect, Option } from "effect";
import {
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import { json, table, tokens } from "../render";
import { agentOpt, forceOpt, scopeOf } from "./stats-options";
import { withScanProgress } from "./stats-progress";

/** Model ids a cell names before it says how many more there are. */
const MODELS_SHOWN = 3;

const FIRST_HEADER: Record<UsageReport["by"], string> = {
  day: "DATE",
  month: "MONTH",
  block: "BLOCK START",
};

/** `YYYY-MM-DD HH:mm` of an ISO time in `tz`. */
const localTime = (iso: string, tz: string): string => {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: tz,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date(iso));
  const part = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((p) => p.type === type)?.value ?? "";
  return `${part("year")}-${part("month")}-${part("day")} ${part("hour")}:${part("minute")}`;
};

const modelsCell = (models: readonly string[]): string => {
  if (models.length === 0) {
    return "-";
  }
  const shown = models.slice(0, MODELS_SHOWN).join(",");
  const more = models.length - MODELS_SHOWN;
  return more > 0 ? `${shown} +${more}` : shown;
};

/** The token, cost and model cells every row and the totals line share. */
const tailCells = (row: UsageTotals): string[] => [
  tokens(row.inputTokens),
  tokens(row.outputTokens),
  tokens(row.cacheCreationTokens),
  tokens(row.cacheReadTokens),
  tokens(row.totalTokens),
  `~$${row.usd.toFixed(2)}`,
  modelsCell(row.models),
];

const keyCells = (row: UsageRow, tz: string): string[] =>
  row.block === null
    ? [row.key]
    : [
        localTime(row.block.startIso, tz),
        localTime(row.block.endIso, tz).slice("YYYY-MM-DD ".length),
        row.block.active ? "*" : "",
      ];

/** The plain-text usage view: header, table, totals line, notes. */
export const renderUsage = (report: UsageReport, compact: boolean): string => {
  const blocks = report.by === "block";
  const headers = [
    FIRST_HEADER[report.by],
    ...(blocks ? ["END", "ACTIVE"] : []),
    "INPUT",
    "OUTPUT",
    "CACHE W",
    "CACHE R",
    "TOTAL",
    "COST",
    "MODELS",
  ];
  const rows = report.rows.map((row) => [
    ...keyCells(row, report.tz),
    ...tailCells(row),
  ]);
  const totals = [
    "TOTAL",
    ...(blocks ? ["", ""] : []),
    ...tailCells(report.totals),
  ];
  const body = rows.length === 0 ? rows : [...rows, totals];
  return [
    `peektrace stats usage  by ${report.by}  ${report.window.days}d  ${report.tz}  ${report.totals.turns} model calls`,
    table(headers, body, { compact }),
    "",
    "NOTES",
    ...report.caveats.map((c) => `- ${c}`),
  ].join("\n");
};

const byOpt = Options.choice("by", ["day", "month", "block"]).pipe(
  Options.withDescription("Bucket by day, month or 5-hour block"),
  Options.withDefault("day" as const)
);

const usageDaysOpt = Options.integer("days").pipe(
  Options.withDescription(
    "Days back from now (default 30 by day, 365 by month, 7 by block)"
  ),
  Options.optional
);

const tzOpt = Options.text("tz").pipe(
  Options.withDescription(
    "IANA time zone for day and month keys (default: this machine's)"
  ),
  Options.optional
);

/** The zone to cut days in: `--tz`, checked, or this machine's own. */
const tzOf = (tz: Option.Option<string>) =>
  Option.match(tz, {
    onNone: () =>
      Effect.succeed(Intl.DateTimeFormat().resolvedOptions().timeZone),
    onSome: (value) =>
      Effect.try({
        try: () => {
          new Intl.DateTimeFormat("en-US", { timeZone: value });
          return value;
        },
        catch: () =>
          new CliUserError({
            message: `Unknown time zone "${value}". Pass an IANA name such as UTC or Europe/Berlin.`,
          }),
      }),
  });

/** `stats usage`. */
export const makeStatsUsage = (globals: GlobalsAccessor) =>
  Command.make(
    "usage",
    {
      by: byOpt,
      days: usageDaysOpt,
      agent: agentOpt,
      tz: tzOpt,
      force: forceOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    (opts) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: opts.json, readOnly: opts.readOnly });
        const scope = yield* scopeOf(opts.agent);
        const tz = yield* tzOf(opts.tz);
        const report = yield* withScanProgress(
          withClient(g, (client) =>
            client.stats.usage({
              ...scope,
              by: opts.by,
              tz,
              force: opts.force,
              ...(Option.isSome(opts.days) ? { days: opts.days.value } : {}),
            })
          )
        );
        return yield* Console.log(
          g.json ? json(report) : renderUsage(report, g.compact)
        );
      })
  );
