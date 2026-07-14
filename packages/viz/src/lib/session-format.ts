/** Shared formatting + health-zone helpers for the session visualizations.
 *
 * Holds the numeric/health vocabulary the session charts share: peak-gauge
 * bands, budget stacking order, and token/byte/percent formatting. `CAT_META`
 * (label/color/estimate flag per budget key) is re-exported straight from core
 * so the UI never re-declares it.
 */
import type { BudgetKey } from "@workspace/core/services/sessions/schema";

export { CAT_META } from "@workspace/core/services/sessions/analyze";

/** Floor → top stacking order for the budget bar + timeline. */
export const STACK_ORDER: readonly BudgetKey[] = [
  "system_tools",
  "listings",
  "memory",
  "files",
  "prompts",
  "tool_results",
  "assistant_text",
  "thinking",
  "other",
  "unattributed",
];

/** Shared numeric scales for token/byte/percent formatting + SVG geometry. */
export const MILLION = 1_000_000;
export const THOUSAND = 1000;
export const PERCENT = 100;
/** Default warn/bad health boundary as a fraction of the window. */
const WARN_THRESHOLD = 0.75;

/** A coarse health zone derived from context fill vs the dumb-zone threshold. */
export type Zone = "ok" | "warn" | "bad";

/** Human verdict word per zone. */
export const HEALTH_LABEL: Record<Zone, string> = {
  ok: "Healthy",
  warn: "Degrading",
  bad: "Rotting",
};

/**
 * Classify a context-fill fraction into a health zone. The warn/bad boundary
 * sits at 0.75 by default but is pushed above the dumb-zone threshold when it is
 * set high, so the warn band never collapses.
 */
export const zoneOf = ({
  frac,
  dumbZone,
}: {
  readonly frac: number;
  readonly dumbZone: number;
}): Zone => {
  const warn = dumbZone < WARN_THRESHOLD ? WARN_THRESHOLD : (1 + dumbZone) / 2;
  if (frac < dumbZone) {
    return "ok";
  }
  return frac < warn ? "warn" : "bad";
};

/** Tailwind text/border/bg classes per health zone (matches the gauge fill). */
export const ZONE_CLASSES: Record<Zone, { fill: string; text: string }> = {
  ok: { fill: "bg-emerald-500", text: "text-emerald-400" },
  warn: { fill: "bg-amber-500", text: "text-amber-400" },
  bad: { fill: "bg-red-500", text: "text-red-400" },
};

/** Thousands-separated integer (rounds). */
export const fmt = (n: number): string => Math.round(n).toLocaleString("en-US");

/** Compact `k` notation, e.g. `1.2K`, `1M`. */
export const fmtK = (n: number): string => {
  if (n >= MILLION) {
    return `${(n / MILLION).toFixed(n % MILLION === 0 ? 0 : 1)}M`;
  }
  if (n >= THOUSAND) {
    return `${(n / THOUSAND).toFixed(n % THOUSAND === 0 ? 0 : 1)}K`;
  }
  return String(Math.round(n));
};

/** Percent with one decimal under 10%, else integer. */
const PCT_DECIMAL_CUTOFF = 10;
export const fmtPct = (frac: number): string => {
  const pct = frac * PERCENT;
  return `${pct < PCT_DECIMAL_CUTOFF ? pct.toFixed(1) : Math.round(pct)}%`;
};

/** Human byte size. */
export const fmtBytes = (n: number): string => {
  if (n >= MILLION) {
    return `${(n / MILLION).toFixed(1)} MB`;
  }
  if (n >= THOUSAND) {
    return `${(n / THOUSAND).toFixed(1)} KB`;
  }
  return `${n} B`;
};

/** First line of a string, clipped to `max` chars. */
export const firstLine = ({
  text,
  max = 160,
}: {
  readonly text: string;
  readonly max?: number;
}): string => {
  const line = text.split("\n", 1)[0] ?? "";
  return line.length > max ? `${line.slice(0, max)}…` : line;
};
