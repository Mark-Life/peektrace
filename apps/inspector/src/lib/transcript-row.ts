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

/** What a CHAT leaf renders on top of the table's fields: the chip label
 *  (`title`), the chip icon (`loadedCategory`), and the error tint on a tool
 *  result that stands alone — that one has no separately-compared `state` prop
 *  to fold `isError` into the way a paired card does. */
export const CHAT_ROW_FIELDS = [
  ...ROW_FIELDS,
  "title",
  "loadedCategory",
  "isError",
] as const satisfies readonly (keyof TimelineEvent)[];

/** True when both events would render an identical chat leaf. */
export const sameChatEvent = (a: TimelineEvent, b: TimelineEvent) =>
  CHAT_ROW_FIELDS.every((field) => a[field] === b[field]);

/** The state a memoized chat bubble compares, apart from its event. Declared
 *  structurally rather than imported from the JSX so the comparators stay in the
 *  testable module without a cycle. */
interface ChatLeafState {
  readonly collapseId: string;
  readonly e: TimelineEvent;
  readonly lane: string;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
}

/** Two renders of the same chat bubble. */
export const sameChatRow = (prev: ChatLeafState, next: ChatLeafState) =>
  prev.open === next.open &&
  prev.collapseId === next.collapseId &&
  prev.lane === next.lane &&
  prev.onToggle === next.onToggle &&
  sameChatEvent(prev.e, next.e);

/** The state a memoized chat tool card compares; either half may be absent. */
interface ChatPairState {
  readonly call: TimelineEvent | null;
  readonly collapseId: string;
  readonly name: string;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
  readonly result: TimelineEvent | null;
  readonly state: string;
}

const bothNullOrSame = (a: TimelineEvent | null, b: TimelineEvent | null) =>
  a === null || b === null ? a === b : sameChatEvent(a, b);

/** Two renders of the same call/result card. */
export const sameToolPair = (prev: ChatPairState, next: ChatPairState) =>
  prev.open === next.open &&
  prev.collapseId === next.collapseId &&
  prev.name === next.name &&
  prev.state === next.state &&
  prev.onToggle === next.onToggle &&
  bothNullOrSame(prev.call, next.call) &&
  bothNullOrSame(prev.result, next.result);
