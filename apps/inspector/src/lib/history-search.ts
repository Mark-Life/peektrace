/** History search: the filtered rows, hit stepping and match highlighting.
 *  The server matches the query (`/history/search`); this side filters by
 *  its hits and marks matches in the rendered text. */
import type { EventRow, TranscriptRowRef } from "./chat-lane";
import { type HistorySort, sortRows } from "./history-sort";

/** Rows the history renders: no system events, then kind, query and sort.
 *  A non-empty `query` keeps only `hits`, the positions the server matched. */
export const visibleRows = <E extends EventRow>({
  events,
  hits,
  kind,
  query,
  sort,
}: {
  readonly events: readonly E[];
  readonly hits: ReadonlySet<number> | undefined;
  /** `"all"` or one event kind. */
  readonly kind: string;
  readonly query: string;
  readonly sort: HistorySort;
}) => {
  const searching = query.length > 0 && hits !== undefined;
  const rows: TranscriptRowRef<E>[] = [];
  for (const [pos, e] of events.entries()) {
    if (e.kind === "system" || (kind !== "all" && e.kind !== kind)) {
      continue;
    }
    if (searching && !hits.has(pos)) {
      continue;
    }
    rows.push({ e, pos });
  }
  return sortRows(rows, sort);
};

export type StepDir = 1 | -1;

/** The next hit after `current` in `dir`, wrapping at both ends. An unknown
 *  current starts at the first hit going forward, the last going back. */
export const stepHit = ({
  current,
  dir,
  hits,
}: {
  readonly current: string | null;
  readonly dir: StepDir;
  readonly hits: readonly string[];
}) => {
  if (hits.length === 0) {
    return null;
  }
  const at = current === null ? -1 : hits.indexOf(current);
  if (at < 0) {
    return (dir === 1 ? hits[0] : hits.at(-1)) ?? null;
  }
  return hits[(at + dir + hits.length) % hits.length] ?? null;
};

/** Counter text: the total before the first step, the position after. */
export const matchLabel = ({
  count,
  current,
}: {
  readonly count: number;
  /** 0-based index of the current hit, or -1 for none. */
  readonly current: number;
}) => {
  if (count === 0) {
    return "No matches";
  }
  if (current < 0) {
    return count === 1 ? "1 match" : `${count} matches`;
  }
  return `${current + 1} of ${count}`;
};

/** Non-overlapping `[start, end)` offsets of `needle` (already lowercased) in
 *  `text`. Text whose lowercase form changes length (e.g. `İ`) is skipped, so
 *  offsets never drift off the original string. */
export const textRanges = (text: string, needle: string) => {
  const out: (readonly [number, number])[] = [];
  if (needle.length === 0) {
    return out;
  }
  const lower = text.toLowerCase();
  if (lower.length !== text.length) {
    return out;
  }
  let from = lower.indexOf(needle);
  while (from >= 0) {
    out.push([from, from + needle.length]);
    from = lower.indexOf(needle, from + needle.length);
  }
  return out;
};

/** Every match of `needle` across `nodes`, in order, stopping at `max`. */
export const rangesIn = <N extends { readonly nodeValue: string | null }>({
  max = Number.POSITIVE_INFINITY,
  needle,
  nodes,
}: {
  readonly max?: number;
  readonly needle: string;
  readonly nodes: Iterable<N>;
}) => {
  const out: {
    readonly end: number;
    readonly node: N;
    readonly start: number;
  }[] = [];
  for (const node of nodes) {
    for (const [start, end] of textRanges(node.nodeValue ?? "", needle)) {
      if (out.length >= max) {
        return out;
      }
      out.push({ end, node, start });
    }
  }
  return out;
};
