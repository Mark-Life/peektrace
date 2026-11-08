/** Options and scope parsing shared by the `stats` subcommands. */
import { Options } from "@effect/cli";
import { AGENT_IDS, type AgentId } from "@workspace/core";
import { Effect, type Option } from "effect";
import { parseAgent } from "../agent-scope";

export const agentOpt = Options.text("agent").pipe(
  Options.withDescription(`Agent to scan: ${AGENT_IDS.join("|")}|all`),
  Options.optional
);

export const forceOpt = Options.boolean("force").pipe(
  Options.withDescription(
    "Ignore cached shards and re-extract every transcript"
  )
);

/** Resolve `--agent` to core's scope: absent or `all` scans every agent. */
export const scopeOf = (agent: Option.Option<string>) =>
  parseAgent(agent).pipe(
    Effect.map((id): { agents?: readonly AgentId[] } =>
      id === undefined ? {} : { agents: [id] }
    )
  );
