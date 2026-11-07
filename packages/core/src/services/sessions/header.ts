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
  readonly message?: { readonly content?: unknown; readonly model?: unknown };
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
 * per content block, sharing a `requestId`; `parse.ts` groups turns the same way.
 */
const assistantKey = (o: RawLine, index: number) =>
  typeof o.requestId === "string" ? `req-${o.requestId}` : `line-${index}`;

/** A user line that only carries tool output, not a message the user wrote.
 * Pi and Codex headers leave tool output out of the count too. */
const isToolResultOnly = (o: RawLine) => {
  const content = o.message?.content;
  return (
    Array.isArray(content) &&
    content.length > 0 &&
    content.every(
      (b) =>
        typeof b === "object" &&
        b !== null &&
        (b as { type?: unknown }).type === "tool_result"
    )
  );
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
    if (o.type === "user" && !isToolResultOnly(o)) {
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
