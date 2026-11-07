/** Token and tool usage read from a Claude assistant line's `usage` object. */
import type { Turn } from "./schema";

/** Turn fields a later streamed line of the same request can raise. */
type StreamedUsage = Pick<
  Turn,
  "outputTokens" | "thinkingTokens" | "webSearchRequests" | "webFetchRequests"
>;

/** Spread helper that drops a key when its value is undefined (exactOptional safe). */
export const opt = <K extends string, V>(
  key: K,
  value: V | undefined
): Partial<Record<K, V>> =>
  value === undefined ? {} : ({ [key]: value } as Record<K, V>);

/** A number, or `undefined` for anything else. */
export const num = (v: unknown) => (typeof v === "number" ? v : undefined);

/** The thinking share of a call's output tokens, when usage reports it. */
export const thinkingTokensOf = (usage: Record<string, unknown>) => {
  const details = usage.output_tokens_details as
    | Record<string, unknown>
    | undefined;
  const n = details?.thinking_tokens;
  return typeof n === "number" ? n : undefined;
};

/** Server tool counts a call reports, keeping only the ones it actually used. */
export const serverToolCounts = (usage: Record<string, unknown>) => {
  const counts = (usage.server_tool_use ?? {}) as Record<string, unknown>;
  const positive = (v: unknown) =>
    typeof v === "number" && v > 0 ? v : undefined;
  return {
    ...opt("webSearchRequests", positive(counts.web_search_requests)),
    ...opt("webFetchRequests", positive(counts.web_fetch_requests)),
  };
};

/**
 * The 1-hour vs 5-minute split of a call's cache writes. Keeps a reported 0,
 * so "none written" stays distinct from "not reported".
 */
export const cacheWriteSplit = (usage: Record<string, unknown>) => {
  const split = (usage.cache_creation ?? {}) as Record<string, unknown>;
  return {
    ...opt("cacheCreation1hTokens", num(split.ephemeral_1h_input_tokens)),
    ...opt("cacheCreation5mTokens", num(split.ephemeral_5m_input_tokens)),
  };
};

/** Max of two optional counts; undefined only when both are. */
const maxOf = (a: number | undefined, b: number | undefined) => {
  if (a === undefined) {
    return b;
  }
  return b === undefined ? a : Math.max(a, b);
};

/**
 * Fold a later streamed line of the same request into its turn. Output-side
 * counts grow as the response streams, so keep the max; input and cache
 * counts are fixed per request and stay as first seen.
 */
export const mergeStreamedUsage = <T extends StreamedUsage>(
  prev: T,
  usage: Record<string, unknown>
) => {
  const output = usage.output_tokens;
  const tools = serverToolCounts(usage);
  return {
    ...prev,
    outputTokens: Math.max(
      prev.outputTokens,
      typeof output === "number" ? output : 0
    ),
    ...opt(
      "thinkingTokens",
      maxOf(prev.thinkingTokens, thinkingTokensOf(usage))
    ),
    ...opt(
      "webSearchRequests",
      maxOf(prev.webSearchRequests, tools.webSearchRequests)
    ),
    ...opt(
      "webFetchRequests",
      maxOf(prev.webFetchRequests, tools.webFetchRequests)
    ),
  };
};
