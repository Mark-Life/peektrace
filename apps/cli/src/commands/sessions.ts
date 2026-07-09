/** `peektrace sessions` subcommands: `ls` and `analyze`.
 *
 * Both run against the shared RPC client (in-process or `--remote`). Tables go to
 * stdout by default; `--json` emits the raw RPC payload. Only Claude is wired —
 * `--agent` is accepted for forward-compat and anything but `claude` lists empty.
 */
import { Args, Command, Options } from "@effect/cli";
import { Console, Effect, Option } from "effect";
import {
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { bytes, json, percent, shortId, table, tokens } from "../render";

const agentOpt = Options.text("agent").pipe(
  Options.withDescription("Agent to list (only `claude` is supported)"),
  Options.optional
);
const projectOpt = Options.text("project").pipe(
  Options.withDescription("Filter sessions by project slug"),
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
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    ({ agent, project, json: jsonFlag, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: jsonFlag, readOnly });
        const agentId = Option.getOrElse(agent, () => "claude");
        if (agentId !== "claude") {
          return yield* Console.log(
            g.json
              ? "[]"
              : `No sessions: agent "${agentId}" is not supported yet.`
          );
        }
        const headers = yield* withClient(g, (client) =>
          client.sessions.list({
            ...(Option.isSome(project) ? { project: project.value } : {}),
          })
        );
        if (g.json) {
          return yield* Console.log(json(headers));
        }
        const rows = headers.map((h) => [
          shortId(h.id),
          h.project,
          h.model ?? "-",
          String(h.messageCount),
          bytes(h.sizeBytes),
          h.updatedAt ?? "-",
          h.title ?? "-",
        ]);
        return yield* Console.log(
          table(
            ["ID", "PROJECT", "MODEL", "MSGS", "SIZE", "UPDATED", "TITLE"],
            rows,
            { compact: g.compact }
          )
        );
      })
  );

/** `sessions analyze <id>` — print the context-budget forensics summary. */
export const makeSessionsAnalyze = (globals: GlobalsAccessor) =>
  Command.make(
    "analyze",
    {
      id: Args.text({ name: "id" }),
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    ({ id, json: jsonFlag, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ json: jsonFlag, readOnly });
        const a = yield* withClient(g, (client) =>
          client.sessions.analyze({ id })
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
