/** Which turn each event belongs to, and where the dumb zone was crossed. */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";

/** Map every event to the 1-based turn it belongs to (for grouping). */
export const turnNumbers = (a: AnalyzedSession): number[] => {
  const reqToTurn = new Map(
    a.turns.map((t, i) => [t.requestId, i + 1] as const)
  );
  const out: number[] = [];
  let cur = 0;
  for (const e of a.events) {
    if (e.requestId && reqToTurn.has(e.requestId)) {
      cur = reqToTurn.get(e.requestId) ?? cur;
    }
    out.push(cur);
  }
  return out;
};

/** The first event of the turn that crossed into the dumb zone, or -1. */
export const crossEvtIdx = (a: AnalyzedSession) =>
  a.dumbZoneCrossTurn >= 0
    ? a.events.findIndex(
        (e) => e.requestId === a.turns[a.dumbZoneCrossTurn]?.requestId
      )
    : -1;
