/**
 * Lazy session-header extraction: pulls the few fields the list view needs
 * (cwd, branch, model, timestamps, message count) without building the full
 * event timeline or running budget analysis.
 */
import type { AgentId } from "../agent-id";
import { isRealModel } from "./claude-records";
import { scanJsonl } from "./parse";
import type { SessionHeader } from "./schema";

interface RawLine {
  readonly aiTitle?: unknown;
  readonly cwd?: unknown;
  readonly gitBranch?: unknown;
  readonly message?: { readonly id?: unknown; readonly model?: unknown };
  readonly requestId?: unknown;
  readonly timestamp?: unknown;
  readonly type?: unknown;
}

/** Mutable header fields gathered while scanning lines. */
interface HeaderAcc {
  cwd?: string;
  gitBranch?: string;
  model?: string;
  startedAt?: string;
  title?: string;
  updatedAt?: string;
}

/**
 * Identity of one assistant API message. Claude writes one response as one line
 * per content block, sharing a `requestId` (as `parse.ts` groups turns).
 */
const assistantKey = (o: RawLine, index: number) => {
  if (typeof o.requestId === "string") {
    return `req-${o.requestId}`;
  }
  return typeof o.message?.id === "string"
    ? `msg-${o.message.id}`
    : `line-${index}`;
};

/** Fold one raw line's header-relevant fields into the accumulator. */
const applyLine = (acc: HeaderAcc, o: RawLine) => {
  const ts = typeof o.timestamp === "string" ? o.timestamp : undefined;
  if (ts) {
    acc.startedAt ??= ts;
    acc.updatedAt = ts;
  }
  if (!acc.cwd && typeof o.cwd === "string") {
    acc.cwd = o.cwd;
  }
  if (typeof o.gitBranch === "string") {
    acc.gitBranch = o.gitBranch;
  }
  if (
    !acc.model &&
    o.type === "assistant" &&
    typeof o.message?.model === "string" &&
    isRealModel(o.message.model)
  ) {
    acc.model = o.message.model;
  }
  if (o.type === "ai-title" && typeof o.aiTitle === "string") {
    acc.title = o.aiTitle;
  }
};

/**
 * Build a lightweight header from raw transcript text. Scans lines for header
 * fields and the first model; never constructs timeline events.
 */
export const buildHeader = (args: {
  readonly text: string;
  readonly id: string;
  readonly slug: string;
  readonly path: string;
  readonly sizeBytes: number;
  readonly mtimeMs: number;
}): SessionHeader => {
  const { text, id, slug, path, sizeBytes, mtimeMs } = args;
  const acc: HeaderAcc = {};
  const assistants = new Set<string>();
  let users = 0;
  const { rows, malformed } = scanJsonl(text);
  rows.forEach((row, index) => {
    if (!row || typeof row !== "object") {
      return;
    }
    const o = row as RawLine;
    applyLine(acc, o);
    if (o.type === "user") {
      users += 1;
    } else if (o.type === "assistant") {
      assistants.add(assistantKey(o, index));
    }
  });

  return {
    id,
    agent: "claude" as AgentId,
    path,
    project: slug,
    messageCount: users + assistants.size,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...(acc.cwd ? { cwd: acc.cwd } : {}),
    ...(acc.gitBranch ? { gitBranch: acc.gitBranch } : {}),
    ...(acc.model ? { model: acc.model } : {}),
    ...(acc.title ? { title: acc.title } : {}),
    ...(acc.startedAt ? { startedAt: acc.startedAt } : {}),
    ...(malformed > 0 ? { malformedLines: malformed } : {}),
  };
};
