/** Lightweight Pi list header: scans raw lines without building a timeline. */
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
  for (const line of rows) {
    if (!line || typeof line !== "object") {
      continue;
    }
    applyHeaderLine(acc, line);
    if (isMessage(line)) {
      messageCount += 1;
    }
  }

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
  };
};
