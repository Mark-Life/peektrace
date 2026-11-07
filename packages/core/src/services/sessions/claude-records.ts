/** Claude record types and fields read outside the main line handlers. */

/** Model id Claude Code writes on harness-made messages (API errors, etc.). */
export const SYNTHETIC_MODEL = "<synthetic>";

/** True for a model id that names a real model call. */
export const isRealModel = (model: string) => model !== SYNTHETIC_MODEL;

/** Record types the parser skips on purpose, so they are not counted as unknown. */
export const KNOWN_IGNORED_TYPES: ReadonlySet<string> = new Set([
  "api-request",
  "last-prompt",
  "queue-operation",
  "file-history-snapshot",
  "mode",
  "permission-mode",
  "session-owner",
  "custom-title",
  "tools-used",
  "agent-name",
  "api-request-blob",
  "api-request-shape",
  "atis-latch",
  "file-history-delta",
  "pr-link",
]);

/** `system` subtypes the parser knows; others are counted as unknown. */
export const KNOWN_SYSTEM_SUBTYPES: ReadonlySet<string> = new Set([
  "compact_boundary",
  "stop_hook_summary",
  "turn_duration",
  "informational",
  "away_summary",
  "local_command",
]);

/** Add one to `key`'s count. */
export const bump = (counts: Map<string, number>, key: string) => {
  counts.set(key, (counts.get(key) ?? 0) + 1);
};

/** A count map as a plain record, or `undefined` when empty. */
export const countsRecord = (counts: ReadonlyMap<string, number>) =>
  counts.size > 0 ? Object.fromEntries(counts) : undefined;

/** Context size before and after a compaction, from `compact_boundary`. */
export interface CompactTokens {
  readonly compactPostTokens?: number;
  readonly compactPreTokens?: number;
}

const num = (v: unknown) => (typeof v === "number" ? v : undefined);

/** The pre/post token counts of a `compact_boundary` line's `compactMetadata`. */
export const compactMetaOf = (o: Record<string, unknown>): CompactTokens => {
  const m = (o.compactMetadata ?? {}) as Record<string, unknown>;
  const pre = num(m.preTokens);
  const post = num(m.postTokens);
  return {
    ...(pre === undefined ? {} : { compactPreTokens: pre }),
    ...(post === undefined ? {} : { compactPostTokens: post }),
  };
};

const ONE_MILLION_SUFFIX = "[1m]";

/** True when a `cost-state` line bills a model under its 1M-context id. */
export const hasOneMillionKey = (o: Record<string, unknown>) => {
  const usage = o.modelUsage;
  return (
    typeof usage === "object" &&
    usage !== null &&
    Object.keys(usage).some((k) => k.endsWith(ONE_MILLION_SUFFIX))
  );
};
