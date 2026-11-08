/** `peektrace stats` — cross-session friction: what to fix first, which named
 * causes cost retries, what fails, and what costs time.
 *
 * Ranked tables over every agent's transcripts, all computed in core and
 * printed here without a single number being re-derived. `--json` emits the
 * versioned `peektrace-stats/v2` report unchanged, so an agent can decode it and
 * act on it; a shape change is a `schemaVersion` change.
 */
import { Command, Options } from "@effect/cli";
import type { AgentId } from "@workspace/core";
import {
  type PromptTarget,
  statsPrompt,
} from "@workspace/core/services/stats/prompts";
import { Console, Effect, Option } from "effect";
import {
  type Globals,
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import { json, table } from "../render";
import { splitRemote } from "../serve-auth";
import {
  STATS_DRILL_SUMMARY,
  STATS_PROMPT_SUMMARY,
  STATS_REFRESH_SUMMARY,
  STATS_SUMMARY,
} from "../summaries";
import { agentOpt, forceOpt, scopeOf } from "./stats-options";
import { withScanProgress } from "./stats-progress";
import { FORMAT, renderDrill, renderReport } from "./stats-render";
import { makeStatsUsage } from "./stats-usage";

const daysOpt = Options.integer("days").pipe(
  Options.withDescription("History window in days (default 60)"),
  Options.optional
);
const limitOpt = Options.integer("limit").pipe(
  Options.withDescription("Rows kept per table (default 20)"),
  Options.optional
);

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
        const result = yield* withScanProgress(
          withClient(g, (client) =>
            client.stats.drill({
              ...scope,
              ...windowOf(opts.days, opts.limit),
              key: target.key,
              table: target.table,
            })
          )
        );
        return yield* Console.log(
          g.json ? json(result) : renderDrill(result, g.compact)
        );
      })
  ).pipe(Command.withDescription(STATS_DRILL_SUMMARY));

/** `stats prompt` — the investigation prompt for the report or one row, the
 * same text the inspector's "Copy prompt" buttons copy, so an agent in a
 * terminal can fetch it itself. */
const makeStatsPrompt = (globals: GlobalsAccessor) =>
  Command.make(
    "prompt",
    {
      cause: Options.text("cause").pipe(
        Options.withDescription(
          "Cause id or key, e.g. zsh-glob-flag (default: the whole report)"
        ),
        Options.optional
      ),
      fail: Options.text("fail").pipe(
        Options.withDescription("Fail-table key"),
        Options.optional
      ),
      time: Options.text("time").pipe(
        Options.withDescription("Time-table job"),
        Options.optional
      ),
      days: daysOpt,
      agent: agentOpt,
      limit: limitOpt,
      readOnly: localReadOnlyOpt,
    },
    (opts) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: false, readOnly: opts.readOnly });
        const picked = (
          [
            ["cause", opts.cause],
            ["fail", opts.fail],
            ["time", opts.time],
          ] as const
        ).flatMap(([table_, key]) =>
          Option.isSome(key) ? [{ table: table_, key: key.value }] : []
        );
        if (picked.length > 1) {
          return yield* new CliUserError({
            message: "Pass at most one of --cause, --fail or --time.",
          });
        }
        const target: PromptTarget = picked[0] ?? { table: "report" };
        const scope = yield* scopeOf(opts.agent);
        const report = yield* withScanProgress(
          withClient(g, (client) =>
            client.stats.report({
              ...scope,
              ...windowOf(opts.days, opts.limit),
            })
          )
        );
        const text = statsPrompt({
          agent: scope.agents?.[0] ?? null,
          format: FORMAT,
          remote: g.remote === undefined ? null : splitRemote(g.remote).url,
          report,
          target,
        });
        if (text === null) {
          return yield* new CliUserError({
            message: `No ${target.table} row "${"key" in target ? target.key : ""}" in this window's top rows. List the keys with: peektrace stats; for a row ranked past 20, pass the --limit that lists it`,
          });
        }
        return yield* Console.log(text);
      })
  ).pipe(Command.withDescription(STATS_PROMPT_SUMMARY));

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
        const summary = yield* withScanProgress(
          withClient(g, (client) =>
            client.stats.refresh({ ...scope, force: opts.force })
          )
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
  ).pipe(Command.withDescription(STATS_REFRESH_SUMMARY));

/** Run the report and print it in the caller's chosen form. */
const runReport = (args: {
  readonly days: Option.Option<number>;
  readonly g: Globals;
  readonly limit: Option.Option<number>;
  readonly scope: { readonly agents?: readonly AgentId[] };
}) =>
  Effect.gen(function* () {
    const { days, g, limit, scope } = args;
    const result = yield* withScanProgress(
      withClient(g, (client) =>
        client.stats.report({ ...scope, ...windowOf(days, limit) })
      )
    );
    return yield* Console.log(
      g.json ? json(result) : renderReport(result, g.compact)
    );
  });

/** `stats` — the ranked tables, plus `drill`, `prompt`, `refresh` and `usage`. */
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
    Command.withDescription(STATS_SUMMARY),
    Command.withSubcommands([
      makeStatsDrill(globals),
      makeStatsPrompt(globals),
      makeStatsRefresh(globals),
      makeStatsUsage(globals),
    ])
  );
