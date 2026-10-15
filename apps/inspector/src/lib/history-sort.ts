/** Table row sort: which column, which direction, and the pure row ordering.
 *
 * Sorting never renumbers rows: each keeps its original `pos`, so open rows,
 * the turn gutter and deep links survive a re-sort. Ties fall back to
 * transcript order, so equal keys read in the order they happened.
 */
import { eventBadgeLabel } from "@workspace/core/services/sessions/labels";
import { eventTokens } from "@workspace/core/services/sessions/tokens";
import type { TranscriptRowRef } from "./chat-lane";

/** Sortable table columns: transcript position, event type, token cost. */
export type HistorySortKey = "order" | "type" | "tokens";

export type SortDir = "asc" | "desc";

/** Active table sort. */
export interface HistorySort {
  readonly dir: SortDir;
  readonly key: HistorySortKey;
}

/** Transcript order, first event on top. */
export const DEFAULT_HISTORY_SORT: HistorySort = { key: "order", dir: "asc" };

const SORT_KEYS: readonly HistorySortKey[] = ["order", "type", "tokens"];

const isSortKey = (v: unknown): v is HistorySortKey =>
  SORT_KEYS.includes(v as HistorySortKey);

/** Coerce a stored sort; the old `"size"` value becomes tokens, biggest first. */
export const historySortOf = (raw: unknown): HistorySort => {
  if (raw === "size") {
    return { key: "tokens", dir: "desc" };
  }
  if (typeof raw !== "object" || raw === null) {
    return DEFAULT_HISTORY_SORT;
  }
  const { key, dir } = raw as Partial<Record<keyof HistorySort, unknown>>;
  if (!isSortKey(key)) {
    return DEFAULT_HISTORY_SORT;
  }
  return { key, dir: dir === "desc" ? "desc" : "asc" };
};

export const sameSort = (x: HistorySort, y: HistorySort) =>
  x.key === y.key && x.dir === y.dir;

/** Header click: the active column flips; a new column starts at its natural
 *  direction (tokens biggest first, the rest ascending). */
export const nextSort = (current: HistorySort, key: HistorySortKey) => {
  if (current.key === key) {
    return { key, dir: current.dir === "asc" ? "desc" : "asc" } as const;
  }
  return { key, dir: key === "tokens" ? "desc" : "asc" } as const;
};

const compareKey = (key: HistorySortKey) => {
  switch (key) {
    case "tokens":
      return (x: TranscriptRowRef, y: TranscriptRowRef) =>
        eventTokens(x.e).tokens - eventTokens(y.e).tokens;
    case "type":
      return (x: TranscriptRowRef, y: TranscriptRowRef) =>
        eventBadgeLabel(x.e).localeCompare(eventBadgeLabel(y.e));
    default:
      return (x: TranscriptRowRef, y: TranscriptRowRef) => x.pos - y.pos;
  }
};

/** Rows in the requested order; ties keep transcript order. Never mutates. */
export const sortRows = <R extends TranscriptRowRef>(
  rows: readonly R[],
  sort: HistorySort
) => {
  if (sameSort(sort, DEFAULT_HISTORY_SORT)) {
    return rows;
  }
  const cmp = compareKey(sort.key);
  const sign = sort.dir === "asc" ? 1 : -1;
  return [...rows].sort((x, y) => sign * cmp(x, y) || x.pos - y.pos);
};
