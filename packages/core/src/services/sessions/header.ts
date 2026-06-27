/**
 * Lazy session-header extraction: pulls the few fields the list view needs
 * (cwd, branch, model, timestamps, message count) without building the full
 * event timeline or running budget analysis.
 */
import type { SessionHeader } from "./schema";

interface RawLine {
  readonly aiTitle?: unknown;
  readonly cwd?: unknown;
  readonly gitBranch?: unknown;
  readonly message?: { readonly model?: unknown };
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

const parseLine = (line: string): RawLine | null => {
  try {
    return JSON.parse(line) as RawLine;
  } catch {
    return null;
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
    typeof o.message?.model === "string"
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
  let messageCount = 0;
  for (const raw of text.split("\n")) {
    if (!raw.trim()) {
      continue;
    }
    messageCount += 1;
    const o = parseLine(raw);
    if (o) {
      applyLine(acc, o);
    }
  }

  return {
    id,
    path,
    project: slug,
    messageCount,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...(acc.cwd ? { cwd: acc.cwd } : {}),
    ...(acc.gitBranch ? { gitBranch: acc.gitBranch } : {}),
    ...(acc.model ? { model: acc.model } : {}),
    ...(acc.title ? { title: acc.title } : {}),
    ...(acc.startedAt ? { startedAt: acc.startedAt } : {}),
  };
};
