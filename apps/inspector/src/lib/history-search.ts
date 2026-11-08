/** History search: the full-text index, the filtered rows, and hit stepping.
 *
 * Search reads an event's whole text (title, preview and body), lowercased
 * once per transcript so each keystroke is a plain `includes` scan. With
 * redaction on the body is already redacted, so a query never matches text the
 * reader cannot see.
 */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { TranscriptRowRef } from "./chat-lane";
import { type HistorySort, sortRows } from "./history-sort";

/** Lowercased searchable text per event, aligned with `events` by position. */
export const searchIndexOf = (events: readonly TimelineEvent[]) =>
  events.map((e) => `${e.title}\n${e.preview}\n${e.body}`.toLowerCase());

/** Rows the history renders: no system events, then kind, query and sort. */
export const visibleRows = ({
  events,
  index,
  kind,
  query,
  sort,
}: {
  readonly events: readonly TimelineEvent[];
  readonly index: readonly string[];
  /** `"all"` or one event kind. */
  readonly kind: string;
  readonly query: string;
  readonly sort: HistorySort;
}) => {
  const needle = query.toLowerCase();
  const rows: TranscriptRowRef[] = [];
  for (const [pos, e] of events.entries()) {
    if (e.kind === "system" || (kind !== "all" && e.kind !== kind)) {
      continue;
    }
    if (needle.length > 0 && !(index[pos] ?? "").includes(needle)) {
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
