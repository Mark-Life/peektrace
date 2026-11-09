/** `peektrace sessions search <q>` — find sessions by text.
 *
 * The scan runs behind `sessions.search` (in-process or `--remote`), newest
 * sessions first, redacted by default. `--json` emits the raw RPC result.
 */
import { Args, Command, Options } from "@effect/cli";
import type { SearchSession, SessionSearchResult } from "@workspace/core";
import { Console, Effect, Option } from "effect";
import { parseAgent } from "../agent-scope";
import {
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import { clipToWidth, json, shortId, table, unreadableNotice } from "../render";
import { SESSIONS_SEARCH_SUMMARY } from "../summaries";
import {
  agentOpt,
  type LsInputNames,
  noRedactOpt,
  parseLsBounds,
  projectOpt,
  sinceOpt,
} from "./sessions";

export const SEARCH_DEFAULT_LIMIT = 20;
const SEARCH_FLAGS: LsInputNames = { limit: "--limit", since: "--since" };

const limitOpt = Options.integer("limit").pipe(
  Options.withDescription("Max sessions, newest first (0 = all)"),
  Options.withDefault(SEARCH_DEFAULT_LIMIT)
);

/** Validate the query, `--limit` and `--since`; `--since` becomes an ISO time. */
export const parseSearchArgs = (
  q: string,
  limit: number,
  since: Option.Option<string>,
  names = SEARCH_FLAGS
) =>
  Effect.gen(function* () {
    if (q.trim() === "") {
      return yield* new CliUserError({
        message: "Give a non-empty search text.",
      });
    }
    const ms = yield* parseLsBounds(limit, since, names);
    return ms === undefined ? undefined : new Date(ms).toISOString();
  });

const BLANK = ["", "", "", "", ""] as const;

const PRETTY_PROJECT_WIDTH = 16;
const DATE_LENGTH = 10;

/**
 * One row per hit; the session columns fill only the first. The pretty table
 * shortens project and date so the fixed columns fit a narrow terminal.
 */
const sessionRows = (s: SearchSession, pretty: boolean) =>
  s.hits.map((hit, i) => [
    ...(i === 0
      ? [
          shortId(s.id),
          s.agent,
          pretty ? clipToWidth(s.project, PRETTY_PROJECT_WIDTH) : s.project,
          pretty
            ? (s.updatedAt?.slice(0, DATE_LENGTH) ?? "-")
            : (s.updatedAt ?? "-"),
          String(s.matchCount),
        ]
      : BLANK),
    `L${hit.line}  ${hit.snippet}`,
  ]);

/** The hits table: one row per hit, session columns on the first. */
export const renderSearch = (
  sessions: SessionSearchResult["sessions"],
  compact: boolean,
  maxWidth?: number
) =>
  table(
    ["SESSION", "AGENT", "PROJECT", "UPDATED", "MATCHES", "SNIPPET"],
    sessions.flatMap((s) => sessionRows(s, !compact)),
    { compact, ...(maxWidth ? { maxWidth } : {}) }
  );

export const noMatchesNotice = (result: SessionSearchResult) =>
  `no matches in ${result.scannedSessions} sessions`;

export const truncatedNotice = (names = SEARCH_FLAGS) =>
  `more sessions match, use ${names.limit} 0 to show all`;

export const makeSessionsSearch = (globals: GlobalsAccessor) =>
  Command.make(
    "search",
    {
      words: Args.text({ name: "q" }).pipe(
        Args.atLeast(1),
        Args.withDescription("Text to find (case-insensitive phrase)")
      ),
      agent: agentOpt,
      project: projectOpt,
      since: sinceOpt,
      limit: limitOpt,
      noRedact: noRedactOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    ({
      words,
      agent,
      project,
      since,
      limit,
      noRedact,
      json: jsonFlag,
      readOnly,
    }) =>
      Effect.gen(function* () {
        const q = words.join(" ");
        const g = yield* globals({ json: jsonFlag, readOnly });
        const agentId = yield* parseAgent(agent);
        const sinceIso = yield* parseSearchArgs(q, limit, since);
        const result = yield* withClient(g, (client) =>
          client.sessions.search({
            q,
            limit,
            redact: !noRedact,
            ...(agentId === undefined ? {} : { agent: agentId }),
            ...(Option.isSome(project) ? { project: project.value } : {}),
            ...(sinceIso === undefined ? {} : { since: sinceIso }),
          })
        );
        if (result.unreadable > 0) {
          yield* Console.error(unreadableNotice(result.unreadable));
        }
        if (g.json) {
          return yield* Console.log(json(result));
        }
        if (result.sessions.length === 0) {
          return yield* Console.error(noMatchesNotice(result));
        }
        yield* Console.log(
          renderSearch(result.sessions, g.compact, process.stdout.columns)
        );
        if (result.truncated) {
          yield* Console.error(truncatedNotice());
        }
      })
  ).pipe(Command.withDescription(SESSIONS_SEARCH_SUMMARY));
