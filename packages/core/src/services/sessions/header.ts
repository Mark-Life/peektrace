/**
 * Lazy session-header extraction: pulls the few fields the list view needs
 * (cwd, branch, model, timestamps, message count) without building the full
 * event timeline or running budget analysis.
 */
import type { AgentId } from "../agent-id";
import { isRealModel } from "./claude-records";
import { type CostTurn, turnsCost } from "./cost";
import { scanJsonl } from "./parse";
import type { SessionHeader } from "./schema";
import { cacheWriteSplit, mergeStreamedUsage, num } from "./usage";

interface RawLine {
  readonly aiTitle?: unknown;
  readonly cwd?: unknown;
  readonly gitBranch?: unknown;
  readonly isCompactSummary?: unknown;
  readonly isMeta?: unknown;
  readonly message?: {
    readonly content?: unknown;
    readonly model?: unknown;
    readonly usage?: unknown;
  };
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

/** A user line the user did not write: harness-injected (`isMeta`, compact
 * summary) or tool output only. Pi and Codex headers skip these too. */
const isNotUserWritten = (o: RawLine) => {
  if (o.isMeta === true || o.isCompactSummary === true) {
    return true;
  }
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

/**
 * Fold one assistant line's usage into its request, the way `parse.ts` builds
 * turns: input and cache counts as first seen, output as the streamed max.
 * Inline sidechain lines count too, as `analyze` prices them. Non-real models
 * are skipped.
 */
const applyUsage = (args: {
  readonly turns: Map<string, CostTurn>;
  readonly key: string;
  readonly o: RawLine;
}) => {
  const { turns, key, o } = args;
  const usage = o.message?.usage;
  const model = o.message?.model;
  if (!usage || typeof usage !== "object") {
    return;
  }
  const u = usage as Record<string, unknown>;
  const prev = turns.get(key);
  if (prev) {
    turns.set(key, mergeStreamedUsage(prev, u));
    return;
  }
  const turn: CostTurn = {
    model: typeof model === "string" ? model : "unknown",
    inputTokens: num(u.input_tokens) ?? 0,
    cacheReadTokens: num(u.cache_read_input_tokens) ?? 0,
    cacheCreationTokens: num(u.cache_creation_input_tokens) ?? 0,
    outputTokens: num(u.output_tokens) ?? 0,
    ...cacheWriteSplit(u),
  };
  const context =
    turn.inputTokens + turn.cacheReadTokens + turn.cacheCreationTokens;
  if (context > 0 && isRealModel(turn.model)) {
    turns.set(key, turn);
  }
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
  const turns = new Map<string, CostTurn>();
  let users = 0;
  const { rows, malformed } = scanJsonl(text);
  rows.forEach((row, index) => {
    if (!row || typeof row !== "object") {
      return;
    }
    const o = row as RawLine;
    applyLine(acc, o);
    if (o.type === "user" && !isNotUserWritten(o)) {
      users += 1;
    } else if (o.type === "assistant") {
      const key = assistantKey(o, index);
      assistants.add(key);
      applyUsage({ turns, key, o });
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
    ...turnsCost([...turns.values()]),
  };
};
