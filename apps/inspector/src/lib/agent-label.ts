import type { AgentId } from "@workspace/core/services/agent-id";

/** Human label per agent id. */
export const AGENT_LABEL: Record<AgentId, string> = {
  claude: "Claude",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};
