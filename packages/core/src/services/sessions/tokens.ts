/** Token estimation and string helpers used by the transcript parser. */
import type { TimelineEvent } from "./schema";

const CHARS_PER_TOKEN = 4;
const DEFAULT_LINE_CAP = 200;

/**
 * Estimate token count from a string using the chars/4 heuristic.
 * Used for per-item sizing only; headline totals come from real usage metadata.
 */
export const estTokens = (s: string): number =>
  s ? Math.round(s.length / CHARS_PER_TOKEN) : 0;

/**
 * First non-empty line of a string, trimmed and length-capped.
 * @param s - source text
 * @param lineCap - max characters before an ellipsis is appended
 */
export const firstLine = (s: string, lineCap = DEFAULT_LINE_CAP): string => {
  const line = (s || "").split("\n").find((l) => l.trim().length > 0) ?? "";
  const trimmed = line.trim();
  return trimmed.length > lineCap ? `${trimmed.slice(0, lineCap)}…` : trimmed;
};

/** An event's best known size: measured when usage pins it down, else estimated. */
export const eventTokens = (
  e: Pick<TimelineEvent, "tokensEst" | "tokensMeasured">
) => ({
  tokens: e.tokensMeasured ?? e.tokensEst,
  measured: e.tokensMeasured !== undefined,
});

/** Size label for an event: `~` marks a chars/4 estimate, none a measured size. */
export const tokenLabel = (
  e: Pick<TimelineEvent, "tokensEst" | "tokensMeasured">,
  format: (n: number) => string
) => {
  const { tokens, measured } = eventTokens(e);
  return `${measured ? "" : "~"}${format(tokens)}`;
};
