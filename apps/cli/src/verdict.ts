/** Context-health verdict bands shared by `sessions analyze` and `statusline`. */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";

/** Fraction at or above which a session is judged to be actively rotting. */
export const ROTTING_FRACTION = 0.7;

type Fill = Pick<
  AnalyzedSession,
  "contextWindow" | "finalContextTokens" | "peakContextTokens"
>;

const fraction = (tokens: number, window: number) =>
  window > 0 ? tokens / window : 0;

/** Peak context as a fraction of the window; 0 when the window is unknown. */
export const peakFractionOf = (a: Fill) =>
  fraction(a.peakContextTokens, a.contextWindow);

/** Final (current) context as a fraction of the window. */
export const finalFractionOf = (a: Fill) =>
  fraction(a.finalContextTokens, a.contextWindow);

/** Verdict band for a context fraction, mirroring the session-report tiers. */
export const verdict = (contextFraction: number, dumbZone: number) => {
  if (contextFraction < dumbZone) {
    return "Healthy";
  }
  if (contextFraction < ROTTING_FRACTION) {
    return "Degrading";
  }
  return "Rotting";
};
