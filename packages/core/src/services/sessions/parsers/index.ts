/** Per-agent parser registry: maps a parseable `AgentId` to its `SessionParser`.
 *
 * `SessionsService` looks a session's owning agent up here to dispatch header
 * building and full parsing. `opencode` has no parser yet (unsupported).
 */
import type { AgentId } from "../../agent-id";
import { claudeParser } from "./claude";
import { codexParser } from "./codex";
import { opencodeParser } from "./opencode";
import { piParser } from "./pi";
import type { SessionParser } from "./types";

/** Parser per agent, or `undefined` for agents without transcript support. */
export const PARSERS: Partial<Record<AgentId, SessionParser>> = {
  claude: claudeParser,
  codex: codexParser,
  pi: piParser,
  opencode: opencodeParser,
};

export { parseCodexSession } from "./codex";
export { parseOpencodeSession } from "./opencode";
export { parsePiSession } from "./pi";
export type { SessionParser } from "./types";
