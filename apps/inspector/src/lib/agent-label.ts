import type { AgentId } from "@workspace/view-core/api-types.gen";

/** Human label per agent id. */
export const AGENT_LABEL: Record<AgentId, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  pi: "Pi",
};
