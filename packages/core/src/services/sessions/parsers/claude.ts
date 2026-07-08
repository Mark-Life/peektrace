/** Claude-Code parser: adapts the existing `parseClaudeSession` + `buildHeader`
 * into the shared `SessionParser` contract. Subagent folding stays in the
 * service (it needs the FileSystem); this covers only the main transcript.
 */
import { buildHeader } from "../header";
import { parseClaudeSession } from "../parse";
import type { SessionParser } from "./types";

/** The Claude-Code `SessionParser`. */
export const claudeParser: SessionParser = {
  agent: "claude",
  parseSession: ({ text, path, sessionId }) =>
    parseClaudeSession({ text, path, sessionId }),
  buildHeader,
};
