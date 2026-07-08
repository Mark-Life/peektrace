/** Pi (pi.dev) session parser — STUB (filled in by the parser workflow). */
import type { SessionHeader } from "../schema";
import type { BuildHeaderArgs, ParseSessionArgs, SessionParser } from "./types";

/** Placeholder — real implementation lands via the parser workflow. */
export const parsePiSession = ({ path, sessionId }: ParseSessionArgs) => ({
  provider: "pi" as const,
  sessionId,
  path,
  models: [] as string[],
  events: [],
  turns: [],
  compactionIndexes: [],
  subagents: [],
});

/** Placeholder header builder. */
export const buildPiHeader = ({
  id,
  slug,
  path,
  sizeBytes,
  mtimeMs,
}: BuildHeaderArgs): SessionHeader => ({
  id,
  agent: "pi",
  path,
  project: slug,
  messageCount: 0,
  sizeBytes,
  updatedAt: new Date(mtimeMs).toISOString(),
});

/** The Pi `SessionParser`. */
export const piParser: SessionParser = {
  agent: "pi",
  parseSession: parsePiSession,
  buildHeader: buildPiHeader,
};
