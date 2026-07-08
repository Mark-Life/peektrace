/** Codex CLI rollout parser — STUB (filled in by the parser workflow). */
import type { SessionHeader } from "../schema";
import type { BuildHeaderArgs, ParseSessionArgs, SessionParser } from "./types";

/** Placeholder — real implementation lands via the parser workflow. */
export const parseCodexSession = ({ path, sessionId }: ParseSessionArgs) => ({
  provider: "codex" as const,
  sessionId,
  path,
  models: [] as string[],
  events: [],
  turns: [],
  compactionIndexes: [],
  subagents: [],
});

/** Placeholder header builder. */
export const buildCodexHeader = ({
  id,
  slug,
  path,
  sizeBytes,
  mtimeMs,
}: BuildHeaderArgs): SessionHeader => ({
  id,
  agent: "codex",
  path,
  project: slug,
  messageCount: 0,
  sizeBytes,
  updatedAt: new Date(mtimeMs).toISOString(),
});

/** The Codex `SessionParser`. */
export const codexParser: SessionParser = {
  agent: "codex",
  parseSession: parseCodexSession,
  buildHeader: buildCodexHeader,
};
