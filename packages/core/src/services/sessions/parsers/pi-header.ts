/** Lightweight Pi list header: scans raw lines without building a timeline. */
import { type CostTurn, headerCost } from "../cost";
import { scanJsonl } from "../parse";
import type { SessionHeader } from "../schema";
import type { BuildHeaderArgs } from "./types";

type RawLine = Record<string, unknown>;

const opt = <K extends string, V>(
  key: K,
  value: V | undefined
): Partial<Record<K, V>> =>
  value === undefined ? {} : ({ [key]: value } as Record<K, V>);

/** Mutable header fields gathered while scanning Pi lines. */
interface HeaderAcc {
  cwd?: string;
  model?: string;
  startedAt?: string;
  title?: string;
  updatedAt?: string;
}

/** Fold one raw Pi line's header-relevant fields into the accumulator. */
const applyHeaderLine = (acc: HeaderAcc, line: RawLine) => {
  const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
  if (ts) {
    acc.startedAt ??= ts;
    acc.updatedAt = ts;
  }
  const type = String(line.type ?? "");
  if (type === "session" && typeof line.cwd === "string") {
    acc.cwd = line.cwd;
  }
  if (
    type === "session_info" &&
    typeof line.name === "string" &&
    line.name.trim()
  ) {
    acc.title = line.name;
  }
  if (
    !acc.model &&
    type === "model_change" &&
    typeof line.modelId === "string"
  ) {
    acc.model = line.modelId;
  }
  if (!acc.model && type === "message") {
    const msg = (line.message ?? {}) as RawLine;
    if (msg.role === "assistant" && typeof msg.model === "string") {
      acc.model = msg.model;
    }
  }
};

const count = (v: unknown) => (typeof v === "number" ? v : 0);

/**
 * Record the usage of one assistant `message` line, the way `pi.ts` builds
 * turns: first line per id, and only when its usage occupies context.
 */
const applyUsage = (
  turns: Map<string, CostTurn>,
  line: RawLine,
  index: number
) => {
  const msg = (line.message ?? {}) as RawLine;
  if (line.type !== "message" || msg.role !== "assistant") {
    return;
  }
  const key = typeof line.id === "string" ? line.id : `line-${index}`;
  const usage = (msg.usage ?? {}) as RawLine;
  const turn: CostTurn = {
    model: String(msg.model ?? "unknown"),
    inputTokens: count(usage.input),
    cacheReadTokens: count(usage.cacheRead),
    cacheCreationTokens: count(usage.cacheWrite),
    outputTokens: count(usage.output),
  };
  const context =
    turn.inputTokens + turn.cacheReadTokens + turn.cacheCreationTokens;
  if (!turns.has(key) && context > 0) {
    turns.set(key, turn);
  }
};

/** A user or assistant `message` line (tool results, bash, system excluded). */
const isMessage = (line: RawLine) => {
  const role = ((line.message ?? {}) as RawLine).role;
  return line.type === "message" && (role === "user" || role === "assistant");
};

/**
 * Build a lightweight Pi header from raw transcript text. Scans lines for the
 * cwd, first model, and timestamps; never constructs timeline events.
 */
export const buildPiHeader = (args: BuildHeaderArgs): SessionHeader => {
  const { text, id, slug, path, sizeBytes, mtimeMs } = args;
  const acc: HeaderAcc = {};
  const { rows, malformed } = scanJsonl(text);
  let messageCount = 0;
  const turns = new Map<string, CostTurn>();
  rows.forEach((line, index) => {
    if (!line || typeof line !== "object") {
      return;
    }
    applyHeaderLine(acc, line);
    applyUsage(turns, line, index);
    if (isMessage(line)) {
      messageCount += 1;
    }
  });

  return {
    id,
    agent: "pi",
    path,
    project: slug,
    messageCount,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...opt("cwd", acc.cwd),
    ...opt("model", acc.model),
    ...opt("startedAt", acc.startedAt),
    ...opt("title", acc.title),
    ...opt("malformedLines", malformed > 0 ? malformed : undefined),
    ...headerCost([...turns.values()]),
  };
};
