/** Lightweight Codex list header: scans raw rollout lines without building a
 * timeline. */
import type { SessionHeader } from "../schema";
import type { BuildHeaderArgs } from "./types";

type RawLine = Record<string, unknown>;

const opt = <K extends string, V>(
  key: K,
  value: V | undefined
): Partial<Record<K, V>> =>
  value === undefined ? {} : ({ [key]: value } as Record<K, V>);

const asObj = (v: unknown): Record<string, unknown> | undefined =>
  v && typeof v === "object" && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : undefined;

const basename = (p: string | undefined): string =>
  p ? (p.split("/").filter(Boolean).pop() ?? "") : "";

/** Mutable header fields gathered while lazily scanning lines. */
interface HeaderAcc {
  cwd?: string;
  gitBranch?: string;
  model?: string;
  startedAt?: string;
  updatedAt?: string;
}

/** Fold one raw rollout line's header-relevant fields into the accumulator. */
const applyHeaderLine = (acc: HeaderAcc, line: RawLine) => {
  const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
  if (ts) {
    acc.startedAt ??= ts;
    acc.updatedAt = ts;
  }
  const type = String(line.type ?? "");
  const payload = asObj(line.payload);
  if (!payload) {
    return;
  }
  if (type === "session_meta") {
    if (typeof payload.cwd === "string") {
      acc.cwd ??= payload.cwd;
    }
    const git = asObj(payload.git);
    if (git && typeof git.branch === "string") {
      acc.gitBranch ??= git.branch;
    }
  }
  if (type === "turn_context") {
    if (typeof payload.cwd === "string") {
      acc.cwd ??= payload.cwd;
    }
    if (!acc.model && typeof payload.model === "string") {
      acc.model = payload.model;
    }
  }
};

/**
 * Build a lightweight Codex list header from raw rollout text. Scans lines for
 * cwd/branch/model/timestamps without constructing a timeline; the project name
 * is the cwd basename (Codex has no per-project slug dir).
 */
export const buildCodexHeader = ({
  text,
  id,
  slug,
  path,
  sizeBytes,
  mtimeMs,
}: BuildHeaderArgs): SessionHeader => {
  const acc: HeaderAcc = {};
  let messageCount = 0;
  for (const raw of text.split("\n")) {
    if (!raw.trim()) {
      continue;
    }
    messageCount += 1;
    try {
      applyHeaderLine(acc, JSON.parse(raw) as RawLine);
    } catch {
      /* tolerate a partial last line of a live rollout */
    }
  }

  return {
    id,
    agent: "codex",
    path,
    project: basename(acc.cwd) || slug || id,
    messageCount,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...opt("cwd", acc.cwd),
    ...opt("gitBranch", acc.gitBranch),
    ...opt("model", acc.model),
    ...opt("startedAt", acc.startedAt),
  };
};
