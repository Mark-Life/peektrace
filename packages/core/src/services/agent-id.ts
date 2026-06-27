/** The agent identity schema — deliberately node-free.
 *
 * `AgentId` / `AGENT_IDS` are pure `Schema` values with no filesystem or Node
 * imports, so the RPC contract (and therefore the browser bundle) can pull them
 * in without dragging `agents.ts`'s `node:os`/`node:child_process` resolvers
 * along. `agents.ts` re-exports these so existing imports keep working.
 */
import { Schema } from "effect";

/** Schema of the four agents the capability matrix tracks. */
export const AgentId = Schema.Literal("claude", "codex", "pi", "opencode");
export type AgentId = typeof AgentId.Type;

/** Every agent id in matrix order. Source of truth for exhaustive records. */
export const AGENT_IDS = [
  "claude",
  "codex",
  "pi",
  "opencode",
] as const satisfies readonly AgentId[];
