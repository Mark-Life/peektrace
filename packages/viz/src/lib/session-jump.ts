/** A request from the summary to show one place in the transcript: a turn
 *  (1-based), the first compaction, or the first subagent's spawning call. The
 *  host decides how to get there; the charts only say where. */
export type SessionJump =
  | { readonly kind: "turn"; readonly turn: number }
  | { readonly kind: "compaction" }
  | { readonly kind: "subagent" };
