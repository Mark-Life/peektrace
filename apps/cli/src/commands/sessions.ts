/** `peektrace sessions` subcommands: `ls` and `analyze`.
 *
 * Both run against the shared RPC client (in-process or `--remote`). Tables go to
 * stdout by default. `analyze --json` emits the raw RPC payload; `ls --json`
 * emits the same rows the table shows. `ls` lists every agent unless `--agent`
 * narrows it, then filters client-side (`--since`, `--cwd`), sorts newest first
 * and caps at `--limit`.
 */
import { Args, Command, Options } from "@effect/cli";
import { AGENT_IDS } from "@workspace/core";
import { Console, Effect, Option } from "effect";
import { parseAgent } from "../agent-scope";
import {
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import { bytes, json, percent, shortId, table, tokens } from "../render";
import { parseSince, resolveCwd, selectSessions } from "./sessions-filter";

const DEFAULT_LIMIT = 30;

const agentOpt = Options.text("agent").pipe(
  Options.withDescription(
    `Agent to list: ${AGENT_IDS.join("|")}|all (default all)`
  ),
  Options.optional
);
const projectOpt = Options.text("project").pipe(
  Options.withDescription(
    "Filter sessions by project name (Claude/Pi slug, or cwd basename for Codex/OpenCode). Slugs starting with - need --project=<slug>"
  ),
  Options.optional
);
const sinceOpt = Options.text("since").pipe(
  Options.withDescription(
    "Only sessions updated since: ISO date or date-time (2026-01-31 is UTC midnight; no offset means UTC) or 7d, 12h, 2w"
  ),
  Options.optional
);
const cwdOpt = Options.text("cwd").pipe(
  Options.withDescription(
    "Only sessions run in this directory or below (path on this machine)"
  ),
  Options.optional
);
const limitOpt = Options.integer("limit").pipe(
  Options.withDescription("Max sessions to list, newest first (0 = all)"),
  Options.withDefault(DEFAULT_LIMIT)
);

const parentOpt = Options.text("parent").pipe(
  Options.withDescription(
    "Parent session id, when <id> is a subagent transcript (agent-*)"
  ),
  Options.optional
);

/** Peak fraction above which a session is judged to be actively rotting. */
const ROTTING_FRACTION = 0.7;

/** Verdict band for a peak-context fraction, mirroring the session-report tiers. */
const verdict = (peakFraction: number, dumbZone: number): string => {
  if (peakFraction < dumbZone) {
    return "Healthy";
  }
  if (peakFraction < ROTTING_FRACTION) {
    return "Degrading";
  }
  return "Rotting";
};

/** `sessions ls` — list lightweight session headers. */
export const makeSessionsLs = (globals: GlobalsAccessor) =>
  Command.make(
    "ls",
    {
      agent: agentOpt,
      project: projectOpt,
      since: sinceOpt,
      cwd: cwdOpt,
      limit: limitOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    ({ agent, project, since, cwd, limit, json: jsonFlag, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: jsonFlag, readOnly });
        const agentId = yield* parseAgent(agent);
        if (limit < 0) {
          return yield* new CliUserError({
            message: `Invalid --limit ${limit}. Use 0 for all, or a positive number.`,
          });
        }
        const sinceMs = Option.isSome(since)
          ? parseSince(since.value, Date.now())
          : undefined;
        if (Option.isSome(since) && sinceMs === undefined) {
          return yield* new CliUserError({
            message: `Invalid --since "${since.value}". Use an ISO date (2026-01-31) or a relative value (7d, 12h, 2w).`,
          });
        }
        const headers = yield* withClient(g, (client) =>
          client.sessions.list({
            ...(Option.isSome(project) ? { project: project.value } : {}),
            ...(agentId === undefined ? {} : { agent: agentId }),
          })
        );
        const { rows: picked, hidden } = selectSessions(headers, {
          limit,
          since: sinceMs,
          cwd: Option.isSome(cwd) ? resolveCwd(cwd.value) : undefined,
        });
        const hint =
          hidden > 0
            ? Console.error(`${hidden} more hidden, use --limit 0 to show all`)
            : Effect.void;
        if (g.json) {
          yield* Console.log(json(picked));
          return yield* hint;
        }
        const rows = picked.map((h) => [
          shortId(h.id),
          h.agent,
          h.project,
          h.model ?? "-",
          String(h.messageCount),
          bytes(h.sizeBytes),
          h.updatedAt ?? "-",
          h.title ?? "-",
        ]);
        yield* Console.log(
          table(
            [
              "ID",
              "AGENT",
              "PROJECT",
              "MODEL",
              "MSGS",
              "SIZE",
              "UPDATED",
              "TITLE",
            ],
            rows,
            { compact: g.compact }
          )
        );
        yield* hint;
      })
  );

/** `sessions analyze <id>` — print the context-budget forensics summary. */
export const makeSessionsAnalyze = (globals: GlobalsAccessor) =>
  Command.make(
    "analyze",
    {
      id: Args.text({ name: "id" }),
      json: localJsonOpt,
      parent: parentOpt,
      readOnly: localReadOnlyOpt,
    },
    ({ id, json: jsonFlag, parent, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: jsonFlag, readOnly });
        const a = yield* withClient(g, (client) =>
          client.sessions.analyze({
            id,
            ...Option.match(parent, {
              onNone: () => ({}),
              onSome: (p) => ({ parent: p }),
            }),
          })
        );
        if (g.json) {
          return yield* Console.log(json(a));
        }
        const peakFraction =
          a.contextWindow > 0 ? a.peakContextTokens / a.contextWindow : 0;
        const summary = table(
          ["FIELD", "VALUE"],
          [
            ["session", a.sessionId],
            ["verdict", verdict(peakFraction, a.dumbZoneFraction)],
            [
              "peak context",
              `${tokens(a.peakContextTokens)} / ${tokens(a.contextWindow)} (${percent(peakFraction)})`,
            ],
            ["final context", tokens(a.finalContextTokens)],
            ["turns", String(a.turnCount)],
            ["tool calls", String(a.toolCallCount)],
            ["dumb-zone cross turn", String(a.dumbZoneCrossTurn)],
          ],
          { compact: g.compact }
        );
        const budgetRows = a.budget
          .filter((slice) => slice.tokens > 0)
          .map((slice) => [
            slice.label,
            tokens(slice.tokens),
            a.peakContextTokens > 0
              ? percent(slice.tokens / a.peakContextTokens)
              : "-",
          ]);
        const budget = table(["BUDGET (AT PEAK)", "TOKENS", "%"], budgetRows, {
          compact: g.compact,
        });
        return yield* Console.log(`${summary}\n\n${budget}`);
      })
  );
