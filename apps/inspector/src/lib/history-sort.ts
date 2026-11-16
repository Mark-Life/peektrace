/** Table row sort: which column, which direction, and the pure row ordering.
 *
 * Sorting never renumbers rows: each keeps its original `pos`, so open rows,
 * the turn gutter and deep links survive a re-sort. Ties fall back to
 * transcript order, so equal keys read in the order they happened.
 */
import { Array as Arr, Order, Predicate } from "effect";

import { badgeOf } from "./chat-lane";
import type { EventRow, TranscriptRowRef } from "./chat-lane";
import { eventTokens } from "./event-tokens";

type Row = TranscriptRowRef<EventRow>;

/** Sortable table columns: transcript position, event type, token cost. */
export type HistorySortKey = "order" | "type" | "tokens";

export type SortDir = "asc" | "desc";

/** Active table sort. */
export interface HistorySort {
  readonly dir: SortDir;
  readonly key: HistorySortKey;
}

/** Transcript order, first event on top. */
export const DEFAULT_HISTORY_SORT: HistorySort = { dir: "asc", key: "order" };

const SORT_KEYS = new Set<string>([
  "order",
  "type",
  "tokens",
] as const satisfies readonly HistorySortKey[]);

const isSortKey = (v: string): v is HistorySortKey => SORT_KEYS.has(v);

/** Coerce a stored sort; the old `"size"` value becomes tokens, biggest first. */
// oxlint-disable-next-line anti-slop/no-unknown-parameters -- parses untrusted localStorage JSON; this is the boundary
export const historySortOf = (raw: unknown): HistorySort => {
  if (raw === "size") {
    return { dir: "desc", key: "tokens" };
  }

  if (
    !Predicate.isObject(raw) ||
    !("key" in raw) ||
    !Predicate.isString(raw.key) ||
    !isSortKey(raw.key)
  ) {
    return DEFAULT_HISTORY_SORT;
  }

  const dir = "dir" in raw && raw.dir === "desc" ? "desc" : "asc";

  return { dir, key: raw.key };
};

export const sameSort = (x: HistorySort, y: HistorySort) =>
  x.key === y.key && x.dir === y.dir;

/** Rows run in transcript order, first to last: only then does "a later
 *  turn" or a divider at a point in time mean anything. */
export const isForwardOrder = (sort: HistorySort) =>
  sameSort(sort, DEFAULT_HISTORY_SORT);

/** Header click: the active column flips; a new column starts at its natural
 *  direction (tokens biggest first, the rest ascending). */
export const nextSort = (current: HistorySort, key: HistorySortKey) => {
  if (current.key === key) {
    return { dir: current.dir === "asc" ? "desc" : "asc", key } as const;
  }

  return { dir: key === "tokens" ? "desc" : "asc", key } as const;
};

const compareKey = (key: HistorySortKey) => {
  if (key === "tokens") {
    return (x: Row, y: Row) =>
      eventTokens(x.e).tokens - eventTokens(y.e).tokens;
  }

  if (key === "type") {
    return (x: Row, y: Row) => badgeOf(x.e).localeCompare(badgeOf(y.e));
  }

  return (x: Row, y: Row) => x.pos - y.pos;
};

/** Rows in the requested order; ties keep transcript order. Never mutates. */
export const sortRows = <R extends Row>(
  rows: readonly R[],
  sort: HistorySort
): readonly R[] => {
  if (sameSort(sort, DEFAULT_HISTORY_SORT)) {
    return rows;
  }

  const cmp = compareKey(sort.key);
  const sign = sort.dir === "asc" ? 1 : -1;

  return Arr.sort(
    rows,
    Order.make<R>((x, y) => {
      const byKey = sign * cmp(x, y);

      return Order.number(byKey === 0 ? x.pos - y.pos : byKey, 0);
    })
  );
};
