/** The read-only tools `peektrace mcp` serves. Each one calls the same RPC and
 * renderer as its CLI twin, so the text matches `--json`-less CLI output in
 * compact mode. No input has `redact` or `force`: core's default redaction holds. */
import { AgentId } from "@workspace/core";
import { DrillTable } from "@workspace/core/services/stats/schema";
import { Effect, Option, Schema } from "effect";
import type { PeektraceClient } from "../client";
import {
  analyzeSession,
  DEFAULT_LIMIT,
  hiddenHint,
  loadSessionList,
  parseLsBounds,
  renderAnalysis,
  renderSessionList,
} from "../commands/sessions";
import { resolveCwd } from "../commands/sessions-filter";
import { renderDrill, renderReport } from "../commands/stats";
import { json, unreadableNotice } from "../render";
import { defineTool, type McpTool } from "./protocol";

const describe = (description: string) => ({ description });

const Format = Schema.optional(
  Schema.Literal("text", "json").annotations(
    describe(
      "text (default): compact tab-separated tables; json: the same payload as the CLI's --json"
    )
  )
);
type Format = Schema.Schema.Type<typeof Format>;

const AgentScope = Schema.optional(
  Schema.Literal(...AgentId.literals, "all").annotations(
    describe("Agent to scope to (default all)")
  )
);

const Count = Schema.Number.pipe(Schema.int(), Schema.nonNegative());
const Positive = Schema.Number.pipe(Schema.int(), Schema.positive());

const Cwd = Schema.optional(
  Schema.String.annotations(
    describe(
      "Only sessions run in this directory or below; relative paths resolve against the server's cwd"
    )
  )
);

const statsWindow = {
  days: Schema.optional(
    Positive.annotations(describe("History window in days (default 60)"))
  ),
  limit: Schema.optional(
    Positive.annotations(describe("Rows kept per table (default 20)"))
  ),
  agent: AgentScope,
  format: Format,
};

/** Join the tool text with the notes the CLI writes to stderr. */
const withNotes = (text: string, notes: readonly string[]) =>
  notes.length === 0 ? text : `${text}\n\n${notes.join("\n")}`;

const scopeOf = (agent: AgentId | "all" | undefined) =>
  agent === undefined || agent === "all" ? {} : { agents: [agent] };

const pick = <T>(format: Format, data: T, text: (d: T) => string) =>
  format === "json" ? json(data) : text(data);

const sessionsList = defineTool({
  name: "sessions_list",
  description:
    "List local coding-agent sessions (Claude Code, Codex, Pi, OpenCode), newest first: id, agent, project, model, message count, estimated cost, size, update time and title. Use an id with sessions_analyze.",
  input: Schema.Struct({
    agent: AgentScope,
    project: Schema.optional(
      Schema.String.annotations(
        describe(
          "Project name (Claude/Pi slug, or cwd basename for Codex/OpenCode)"
        )
      )
    ),
    since: Schema.optional(
      Schema.String.annotations(
        describe(
          "Only sessions updated since: ISO date (2026-01-31, UTC) or relative 7d, 12h, 2w"
        )
      )
    ),
    cwd: Cwd,
    limit: Schema.optional(
      Count.annotations(
        describe(
          `Max sessions, newest first (default ${DEFAULT_LIMIT}, 0 = all)`
        )
      )
    ),
    format: Format,
  }),
  run: (client: PeektraceClient, input) =>
    Effect.gen(function* () {
      const limit = input.limit ?? DEFAULT_LIMIT;
      const sinceMs = yield* parseLsBounds(
        limit,
        Option.fromNullable(input.since)
      );
      const list = yield* loadSessionList(client, {
        agent: input.agent === "all" ? undefined : input.agent,
        project: input.project,
        since: sinceMs,
        cwd: input.cwd === undefined ? undefined : resolveCwd(input.cwd),
        limit,
      });
      const notes = [
        ...(list.unreadable > 0 ? [unreadableNotice(list.unreadable)] : []),
      ];
      const hint = hiddenHint(list.hidden);
      if (input.format === "json") {
        return withNotes(json(list.picked), [
          ...notes,
          ...(hint === undefined ? [] : [hint]),
        ]);
      }
      const rendered = renderSessionList(list.picked, true);
      return withNotes(rendered.text, [
        ...notes,
        ...rendered.notes,
        ...(hint === undefined ? [] : [hint]),
      ]);
    }),
});

const sessionsAnalyze = defineTool({
  name: "sessions_analyze",
  description:
    "Context-budget forensics for one session: verdict, peak and final context, turns, tool calls, estimated cost and the token budget at peak. Pass an id (short id works), or latest:true, or a cwd to pick the newest session there.",
  input: Schema.Struct({
    id: Schema.optional(
      Schema.String.annotations(
        describe("Session id, short id, or transcript path")
      )
    ),
    latest: Schema.optional(
      Schema.Boolean.annotations(
        describe("Analyze the newest session of any agent")
      )
    ),
    cwd: Cwd,
    parent: Schema.optional(
      Schema.String.annotations(
        describe(
          "Parent session id, when id is a subagent transcript (agent-*)"
        )
      )
    ),
    format: Format,
  }),
  run: (client: PeektraceClient, input) =>
    analyzeSession(client, {
      id: input.id,
      latest: input.latest ?? false,
      cwd: input.cwd === undefined ? undefined : resolveCwd(input.cwd),
      parent: input.parent,
    }).pipe(
      Effect.map((a) => pick(input.format, a, (d) => renderAnalysis(d, true)))
    ),
});

const statsReport = defineTool({
  name: "stats_report",
  description:
    "Cross-session friction report: what to fix first, named causes of retries, what fails and what costs time, with suggested rules for CLAUDE.md / AGENTS.md. Each row has a DRILL cell such as `--cause K`; expand it with stats_drill.",
  input: Schema.Struct(statsWindow),
  run: (client: PeektraceClient, input) =>
    client.stats
      .report({
        ...scopeOf(input.agent),
        ...(input.days === undefined ? {} : { days: input.days }),
        ...(input.limit === undefined ? {} : { limit: input.limit }),
      })
      .pipe(
        Effect.map((r) => pick(input.format, r, (d) => renderReport(d, true)))
      ),
});

const statsDrill = defineTool({
  name: "stats_drill",
  description:
    'The calls behind one stats_report row. A DRILL cell `--cause K` maps to {table:"cause", key:"K"}; likewise --fail, --time, --cluster and --wait.',
  input: Schema.Struct({
    table: DrillTable.annotations(describe("Table the key comes from")),
    key: Schema.String.annotations(
      describe("Key from the DRILL cell, without the --table prefix")
    ),
    offset: Schema.optional(
      Count.annotations(describe("First hit to return (default 0)"))
    ),
    pageSize: Schema.optional(
      Positive.annotations(describe("Hits per page (default 200)"))
    ),
    ...statsWindow,
  }),
  run: (client: PeektraceClient, input) =>
    client.stats
      .drill({
        ...scopeOf(input.agent),
        ...(input.days === undefined ? {} : { days: input.days }),
        ...(input.limit === undefined ? {} : { limit: input.limit }),
        ...(input.offset === undefined ? {} : { offset: input.offset }),
        ...(input.pageSize === undefined ? {} : { pageSize: input.pageSize }),
        table: input.table,
        key: input.key,
      })
      .pipe(
        Effect.map((r) => pick(input.format, r, (d) => renderDrill(d, true)))
      ),
});

/** Every tool the server lists, in display order. */
export const TOOLS: readonly McpTool<PeektraceClient>[] = [
  sessionsList,
  sessionsAnalyze,
  statsReport,
  statsDrill,
];
