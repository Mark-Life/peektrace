/** Token estimation and string helpers used by the transcript parser. */

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
