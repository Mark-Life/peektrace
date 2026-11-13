/** Paging the session history: which event pages the drawn rows need, which
 *  held pages a new summary made stale, and turning index rows back into
 *  whole events once their page is here.
 *
 * The detail loads a body-free index of every event up front, so filtering,
 * sorting, jumps and the chat plan run over all rows at once. Event bodies
 * come in fixed pages of `PAGE_SIZE` positions, fetched as rows are drawn.
 */
import type {
  TimelineEvent,
  TurnSnapshot,
} from "@workspace/core/services/sessions/schema";
import type { GrowthSeries } from "./api-types.gen";
import type {
  ChatNode,
  EventRow,
  RunItem,
  TranscriptRowRef,
} from "./chat-lane";

/** Events per page, as the server cuts `summary.history.pages`. */
export const PAGE_SIZE = 100;

/** Pages fetched past each end of the drawn rows. */
export const PREFETCH = 1;

/** Event pages held so far, by page number. */
export type PageCache = ReadonlyMap<
  number,
  { readonly events: readonly TimelineEvent[]; readonly hash: string }
>;

export const pageOf = (pos: number, size = PAGE_SIZE) => Math.floor(pos / size);

/** Sorted unique page numbers of `positions`. */
export const pagesFor = (positions: Iterable<number>, size = PAGE_SIZE) =>
  [...new Set(Array.from(positions, (p) => pageOf(p, size)))].sort(
    (x, y) => x - y
  );

/** Event positions a chat node or table row draws. */
export const positionsOf = (
  n: ChatNode<EventRow> | TranscriptRowRef<EventRow>
): number[] => {
  if (!("type" in n)) {
    return [n.pos];
  }
  switch (n.type) {
    case "message":
    case "band":
    case "notice":
      return [n.row.pos];
    case "tool":
      return [n.call?.pos, n.result?.pos].filter(
        (p): p is number => p !== undefined
      );
    case "run":
      return n.items.map((i) => i.pos);
    default:
      return [];
  }
};

/** Pages the drawn rows need, plus `prefetch` pages past each end, inside
 *  the `total` events. A list short enough to draw whole reports every row
 *  as drawn, so it loads every page. */
export const neededPages = ({
  drawn,
  prefetch = PREFETCH,
  size = PAGE_SIZE,
  total,
}: {
  /** Positions of the drawn rows. */
  readonly drawn: Iterable<number>;
  readonly prefetch?: number;
  readonly size?: number;
  readonly total: number;
}) => {
  const count = Math.ceil(total / size);
  const pages = pagesFor(drawn, size).filter((p) => p < count);
  const first = pages[0];
  const last = pages.at(-1);
  if (first === undefined || last === undefined) {
    return pages;
  }
  const extra: number[] = [];
  for (let i = 1; i <= prefetch; i++) {
    extra.push(first - i, last + i);
  }
  return [
    ...new Set([...pages, ...extra.filter((p) => p >= 0 && p < count)]),
  ].sort((x, y) => x - y);
};

/** Pages whose hash changed or that are new in `next`. */
export const stalePages = (held: readonly string[], next: readonly string[]) =>
  next.flatMap((hash, i) => (held[i] === hash ? [] : [i]));

/** The whole event at `pos` when its page is held, else null. */
export const mergeRow = (
  pos: number,
  pages: PageCache,
  size = PAGE_SIZE
): TimelineEvent | null =>
  pages.get(pageOf(pos, size))?.events[pos % size] ?? null;

/** The whole event of `row`: its own when it is one, else its page's. */
const wholeOf = (
  row: TranscriptRowRef<EventRow>,
  at: (pos: number) => TimelineEvent | null
) => ("body" in row.e ? row.e : at(row.pos));

const hydrateRow = (
  row: TranscriptRowRef<EventRow>,
  at: (pos: number) => TimelineEvent | null
): TranscriptRowRef | null => {
  const e = wholeOf(row, at);
  return e === null ? null : { e, pos: row.pos };
};

const build = (
  n: ChatNode<EventRow>,
  at: (pos: number) => TimelineEvent | null
): ChatNode | null => {
  switch (n.type) {
    case "message":
    case "band":
    case "notice": {
      const row = hydrateRow(n.row, at);
      return row === null ? null : { type: n.type, row };
    }
    case "tool": {
      const call = n.call === null ? null : hydrateRow(n.call, at);
      const result = n.result === null ? null : hydrateRow(n.result, at);
      const missing =
        (n.call !== null && call === null) ||
        (n.result !== null && result === null);
      return missing ? null : { type: "tool", call, result };
    }
    case "run": {
      const items: RunItem[] = [];
      for (const item of n.items) {
        const e = wholeOf(item, at);
        if (e === null) {
          return null;
        }
        items.push({ e, pos: item.pos, repeat: item.repeat });
      }
      return { type: "run", items };
    }
    default:
      return n;
  }
};

/** Hydrated results kept per index node or row, with the events they used. */
export type HydrateCache = WeakMap<
  object,
  { readonly events: readonly (TimelineEvent | null)[]; readonly out: unknown }
>;

const cached = <I extends ChatNode<EventRow> | TranscriptRowRef<EventRow>, O>(
  item: I,
  at: (pos: number) => TimelineEvent | null,
  cache: HydrateCache,
  make: () => O
): O => {
  const events = positionsOf(item).map(at);
  const held = cache.get(item);
  if (
    held !== undefined &&
    held.events.length === events.length &&
    held.events.every((e, i) => e === events[i])
  ) {
    return held.out as O;
  }
  const out = make();
  cache.set(item, { events, out });
  return out;
};

/** Turns index nodes and rows into whole-event ones, or null while a page
 *  they need loads. A result is reused while its events are the same
 *  objects, so memoized views see the same node until its page changes. */
export const hydrator = (
  eventAt: ((pos: number) => TimelineEvent | null) | undefined,
  cache: HydrateCache = new WeakMap()
) => {
  const at = eventAt ?? (() => null);
  return {
    node: (n: ChatNode<EventRow>) => cached(n, at, cache, () => build(n, at)),
    row: (r: TranscriptRowRef<EventRow>) =>
      cached(r, at, cache, () => hydrateRow(r, at)),
  };
};

/** Column `col` at row `i`; every column has one value per snapshot. */
const cell = (col: readonly number[], i: number) => col[i] as number;

/** The snapshots the growth chart draws, rebuilt from the growth columns. */
export const snapshotsOf = (
  g: Pick<GrowthSeries, "snapshots">
): Pick<TurnSnapshot, "ctx" | "outputTokens" | "slices" | "turnIndex">[] => {
  const { ctx, outputTokens, slices: s, turnIndex } = g.snapshots;
  return turnIndex.map((t, i) => ({
    ctx: cell(ctx, i),
    outputTokens: cell(outputTokens, i),
    slices: {
      system_tools: cell(s.system_tools, i),
      listings: cell(s.listings, i),
      memory: cell(s.memory, i),
      files: cell(s.files, i),
      prompts: cell(s.prompts, i),
      tool_results: cell(s.tool_results, i),
      assistant_text: cell(s.assistant_text, i),
      thinking: cell(s.thinking, i),
      other: cell(s.other, i),
      unattributed: cell(s.unattributed, i),
    },
    turnIndex: t,
  }));
};

/** The usage a chat turn rule shows for 1-based turn `n`. */
export const turnRuleOf = (g: Pick<GrowthSeries, "turns">, n: number) =>
  g.turns[n - 1];
