import type { WideEvent } from "./schema";

export const SLOW_MS = 2000;

/** Flag (not a drop filter — local keeps 100%) for the export default. */
export const isInteresting = (e: WideEvent): boolean =>
  e.outcome !== "success" || e.durationMs > SLOW_MS;
