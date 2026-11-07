/** Shared `--agent` parsing for commands that scope to one coding agent. */
import { AGENT_IDS, type AgentId } from "@workspace/core";
import { Effect, Option } from "effect";
import { CliUserError } from "./errors";

/** Resolve `--agent`: absent or `all` means every agent (`undefined`). */
export const parseAgent = (
  agent: Option.Option<string>
): Effect.Effect<AgentId | undefined, CliUserError> => {
  const value = Option.getOrElse(agent, () => "all");
  if (value === "all") {
    return Effect.succeed(undefined);
  }
  const found = AGENT_IDS.find((id) => id === value);
  return found === undefined
    ? Effect.fail(
        new CliUserError({
          message: `Unknown agent "${value}". Known: ${AGENT_IDS.join(", ")}, all.`,
        })
      )
    : Effect.succeed(found);
};
