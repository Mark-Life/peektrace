/** When two renders of a transcript row are the same row.
 *
 * Re-analyzing a live session rebuilds every `TimelineEvent` object, so a row
 * memoized on object identity would re-render on every agent write — hundreds of
 * rows, each with a highlighted body, several times a minute. Rows are pure in
 * the fields they render, so `sameEvent` compares exactly those instead.
 *
 * Kept beside the JSX rather than inside it so it is unit-testable: a field that
 * `EventRow` renders but this function forgets would leave a visibly stale row.
 */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";

/** The `TimelineEvent` fields a transcript row renders. Adding a field to the
 *  row means adding it here — that is the whole contract. */
export const ROW_FIELDS = [
  "index",
  "kind",
  "toolName",
  "attachmentType",
  "isSidechain",
  "preview",
  "tokensEst",
  "body",
] as const satisfies readonly (keyof TimelineEvent)[];

/** True when both events would render an identical row. */
export const sameEvent = (a: TimelineEvent, b: TimelineEvent) =>
  ROW_FIELDS.every((field) => a[field] === b[field]);
