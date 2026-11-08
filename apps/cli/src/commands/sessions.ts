/** `peektrace sessions` subcommands: `ls` and `analyze` (`export` lives in
 * `sessions-export.ts`).
 *
 * Both run against the shared RPC client (in-process or `--remote`). Tables go to
 * stdout by default. `analyze --json` emits the raw RPC payload; `ls --json`
 * emits the same rows the table shows. `ls` lists every agent unless `--agent`
 * narrows it, then filters client-side (`--since`, `--cwd`), sorts newest first
 * and caps at `--limit`.
 */
import { Args, Command, Options } from "@effect/cli";
import {
  AGENT_IDS,
  type AgentId,
  type AnalyzedSession,
  type SessionCost,
} from "@workspace/core";
import { Console, Effect, Option } from "effect";
import { parseAgent } from "../agent-scope";
import {
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  type PeektraceClient,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import {
  bytes,
  crossTurnCell,
  json,
  percent,
  shortId,
  table,
  tokens,
  unreadableNotice,
  usdCell,
} from "../render";
import { SESSIONS_ANALYZE_SUMMARY, SESSIONS_LS_SUMMARY } from "../summaries";
import { peakFractionOf, verdict } from "../verdict";
import {
  parseSince,
  resolveCwd,
  resolveTargetId,
  type SelectOptions,
  selectSessions,
  type TargetRequest,
} from "./sessions-filter";

export const DEFAULT_LIMIT = 30;

export const agentOpt = Options.text("agent").pipe(
  Options.withDescription(`Agent: ${AGENT_IDS.join("|")}|all (default all)`),
  Options.optional
);
export const projectOpt = Options.text("project").pipe(
  Options.withDescription(
    "Filter sessions by project name (Claude/Pi slug, or cwd basename for Codex/OpenCode). Slugs starting with - need --project=<slug>"
  ),
  Options.optional
);
export const sinceOpt = Options.text("since").pipe(
  Options.withDescription(
    "Only sessions updated since: ISO date or date-time (2026-01-31 is UTC midnight; no offset means UTC) or 7d, 12h, 2w"
  ),
  Options.optional
);
export const cwdOpt = Options.text("cwd").pipe(
  Options.withDescription(
    "Only sessions run in this directory or below (path on this machine)"
  ),
  Options.optional
);
const limitOpt = Options.integer("limit").pipe(
  Options.withDescription("Max sessions to list, newest first (0 = all)"),
  Options.withDefault(DEFAULT_LIMIT)
);

export const noRedactOpt = Options.boolean("no-redact").pipe(
  Options.withDescription(
    "Keep secrets and home paths in the output (redacted by default)"
  )
);

export const parentOpt = Options.text("parent").pipe(
  Options.withDescription(
    "Parent session id, when <id> is a subagent transcript (agent-*)"
  ),
  Options.optional
);

/** Validate `--limit` and parse `--since` into epoch ms. */
export const parseLsBounds = (limit: number, since: Option.Option<string>) =>
  Effect.gen(function* () {
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
    return sinceMs;
  });

export interface ListRequest extends SelectOptions {
  readonly agent?: AgentId | undefined;
  readonly project?: string | undefined;
}

/** Fetch the listing, then filter, sort and cap it client-side. */
export const loadSessionList = (
  client: PeektraceClient,
  { agent, project, ...select }: ListRequest
) =>
  client.sessions
    .listing({
      ...(project === undefined ? {} : { project }),
      ...(agent === undefined ? {} : { agent }),
    })
    .pipe(
      Effect.map(({ headers, unreadable }) => {
        const { rows: picked, hidden } = selectSessions(headers, select);
        return { picked, hidden, unreadable: unreadable.length };
      })
    );

type ListedSession = Effect.Effect.Success<
  ReturnType<typeof loadSessionList>
>["picked"][number];

/** The "more hidden" hint, when `--limit` cut rows. */
export const hiddenHint = (hidden: number) =>
  hidden > 0 ? `${hidden} more hidden, use --limit 0 to show all` : undefined;

/** The `sessions ls` table, plus the notes the CLI prints to stderr. */
export const renderSessionList = (
  picked: readonly ListedSession[],
  compact: boolean
) => {
  const rows = picked.map((h) => [
    shortId(h.id),
    h.agent,
    h.project,
    h.model ?? "-",
    String(h.messageCount),
    `${usdCell(h.costUsd)}${h.unpricedModels ? "*" : ""}`,
    bytes(h.sizeBytes),
    h.updatedAt ?? "-",
    h.title ?? "-",
  ]);
  const text = table(
    [
      "ID",
      "AGENT",
      "PROJECT",
      "MODEL",
      "MSGS",
      "EST COST",
      "SIZE",
      "UPDATED",
      "TITLE",
    ],
    rows,
    { compact }
  );
  const notes = [
    ...(picked.some((h) => h.costUsd !== undefined)
      ? ["EST COST covers the main transcript only, without subagent files"]
      : []),
    ...(picked.some((h) => h.unpricedModels)
      ? ["* some models unpriced: the figure is too low"]
      : []),
  ];
  return { text, notes };
};

const printNotes = (notes: readonly (string | undefined)[]) =>
  Effect.forEach(
    notes.filter((n) => n !== undefined),
    (n) => Console.error(n),
    { discard: true }
  );

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
        const sinceMs = yield* parseLsBounds(limit, since);
        const list = yield* withClient(g, (client) =>
          loadSessionList(client, {
            agent: agentId,
            project: Option.getOrUndefined(project),
            limit,
            since: sinceMs,
            cwd: Option.isSome(cwd) ? resolveCwd(cwd.value) : undefined,
          })
        );
        if (list.unreadable > 0) {
          yield* Console.error(unreadableNotice(list.unreadable));
        }
        const hint = hiddenHint(list.hidden);
        if (g.json) {
          yield* Console.log(json(list.picked));
          return yield* printNotes([hint]);
        }
        const { text, notes } = renderSessionList(list.picked, g.compact);
        yield* Console.log(text);
        yield* printNotes([...notes, hint]);
      })
  ).pipe(Command.withDescription(SESSIONS_LS_SUMMARY));

const latestOpt = Options.boolean("latest").pipe(
  Options.withDescription(
    "Analyze the newest session of any agent (combine with --cwd to scope it)"
  )
);

/** Summary rows for a session's cost; `-` when there is nothing to price. */
const costRows = (
  cost: SessionCost | undefined,
  provider: AnalyzedSession["provider"]
) => {
  const total = usdCell(cost?.totalUsd);
  const fromAgent = cost?.source === "agent" && cost.totalUsd !== undefined;
  const subagents = cost?.subagentsUsd ?? 0;
  const unpriced = cost?.unpricedModels ?? [];
  const unreadable = cost?.unreadableSubagents ?? 0;
  return [
    ["est. cost", fromAgent ? `${total} (${provider})` : total],
    ...(subagents > 0 ? [["  subagents", usdCell(subagents)]] : []),
    ...(unpriced.length > 0 ? [["unpriced models", unpriced.join(", ")]] : []),
    ...(unreadable > 0
      ? [["unreadable subagents", `${unreadable} (not in est. cost)`]]
      : []),
  ];
};

/** Resolve the target (id, `--latest` or `--cwd`) and analyze it. */
export const analyzeSession = (
  client: PeektraceClient,
  request: TargetRequest
) =>
  resolveTargetId(client, request).pipe(
    Effect.flatMap((target) =>
      client.sessions.analyze({
        id: target,
        ...(request.parent === undefined ? {} : { parent: request.parent }),
      })
    )
  );

/** The `sessions analyze` summary and budget tables. */
export const renderAnalysis = (a: AnalyzedSession, compact: boolean) => {
  const peakFraction = peakFractionOf(a);
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
      ...costRows(a.cost, a.provider),
      ["dumb-zone cross turn", crossTurnCell(a.dumbZoneCrossTurn)],
    ],
    { compact }
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
    compact,
  });
  return `${summary}\n\n${budget}`;
};

/** `sessions analyze [<id>] [--latest] [--cwd <dir>]` — print the context-budget forensics summary. */
export const makeSessionsAnalyze = (globals: GlobalsAccessor) =>
  Command.make(
    "analyze",
    {
      id: Args.text({ name: "id" }).pipe(
        Args.withDescription("Session id, short id, or transcript path"),
        Args.optional
      ),
      latest: latestOpt,
      cwd: cwdOpt,
      json: localJsonOpt,
      parent: parentOpt,
      readOnly: localReadOnlyOpt,
    },
    ({ id, latest, cwd, json: jsonFlag, parent, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: jsonFlag, readOnly });
        const a = yield* withClient(g, (client) =>
          analyzeSession(client, {
            id: Option.getOrUndefined(id),
            latest,
            cwd: Option.isSome(cwd) ? resolveCwd(cwd.value) : undefined,
            parent: Option.getOrUndefined(parent),
          })
        );
        return yield* Console.log(
          g.json ? json(a) : renderAnalysis(a, g.compact)
        );
      })
  ).pipe(Command.withDescription(SESSIONS_ANALYZE_SUMMARY));
