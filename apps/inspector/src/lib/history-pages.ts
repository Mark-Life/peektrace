/** Paging the session history: which events the drawn rows need, how to
 *  fetch them, and turning index rows back into whole events once they are
 *  here.
 *
 * The detail loads a body-free index of every event up front, so filtering,
 * sorting, jumps and the chat plan run over all rows at once. Rows drawn in
 * event order get their bodies in fixed pages of `PAGE_SIZE` positions.
 * Rows spread over the transcript (a size sort, scattered search hits) get
 * only their own events, in short runs.
 */
import type {
  TimelineEvent,
  TurnSnapshot,
} from "@workspace/core/services/sessions/schema";
import type { GrowthSeries, HistoryPageMeta } from "./api-types.gen";
import type {
  ChatNode,
  EventRow,
  RunItem,
  TranscriptRowRef,
} from "./chat-lane";

/** Events per page, as the server cuts `summary.history.pages`. */
export const PAGE_SIZE = 100;

/** Most events in one `/history` request, as the server allows. */
const MAX_RUN = 500;

/** Pages fetched past each end of rows drawn in event order. */
const PREFETCH = 1;

/** Drawn rows over more than `SPREAD_PAGES` pages, spanning more than
 *  `SPREAD` positions per row, are spread out: they fetch their own events,
 *  not whole pages. */
const SPREAD = 4;
const SPREAD_PAGES = 2;

/** Body UTF-16 units a run may fetch between two drawn rows to join them. */
const JOIN_BUDGET = 4096;
/** What an event costs beyond its body (title, preview, fields), in the
 *  same units. */
const ROW_COST = 256;

/** Event bodies held so far. */
export interface BodyCache {
  /** Whole pages by page number, with their hash. */
  readonly pages: ReadonlyMap<
    number,
    { readonly events: readonly TimelineEvent[]; readonly hash: string }
  >;
  /** Events fetched outside whole pages, by position, with the `rev` they
   *  came at. A page that arrives drops the ones it holds, so a row here is
   *  newer than its page. */
  readonly rows: ReadonlyMap<
    number,
    { readonly event: TimelineEvent; readonly rev: string }
  >;
}

export const EMPTY_BODIES: BodyCache = { pages: new Map(), rows: new Map() };

/** What the drawn rows need: whole pages, or single positions. */
export interface Needed {
  readonly pages: readonly number[];
  readonly rows: readonly number[];
}

export const NOTHING_NEEDED: Needed = { pages: [], rows: [] };

/** One `/history` request: a whole page checked against its hash, or a run
 *  of positions checked against the summary `rev`. */
export type BodyFetch =
  | { readonly kind: "page"; readonly page: number; readonly hash: string }
  | {
      readonly kind: "run";
      readonly offset: number;
      readonly limit: number;
      readonly rev: string;
    };

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
 *  the `total` events. */
const neededPages = ({
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

/** Rows drawn in event order need their pages and `prefetch` more each
 *  way. Rows spread over the transcript need only their own positions. A
 *  list short enough to draw whole reports every row as drawn. */
export const neededBodies = ({
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
}): Needed => {
  const rows = [...new Set(drawn)]
    .filter((p) => p >= 0 && p < total)
    .sort((x, y) => x - y);
  const first = rows[0];
  const last = rows.at(-1);
  if (first === undefined || last === undefined) {
    return NOTHING_NEEDED;
  }
  const spread =
    last - first + 1 > rows.length * SPREAD &&
    pagesFor(rows, size).length > SPREAD_PAGES;
  return spread
    ? { pages: [], rows }
    : { pages: neededPages({ drawn: rows, prefetch, size, total }), rows: [] };
};

/** Sorted `positions` as runs of at most `max` events. Two neighbours join
 *  when the events between them fit in `budget` UTF-16 units. */
const runsOf = (
  positions: readonly number[],
  {
    bodyLen,
    budget = JOIN_BUDGET,
    max = MAX_RUN,
  }: {
    readonly bodyLen: (pos: number) => number;
    readonly budget?: number;
    readonly max?: number;
  }
) => {
  const gapFits = (from: number, to: number) => {
    let sum = 0;
    for (let p = from + 1; p < to && sum <= budget; p++) {
      sum += bodyLen(p) + ROW_COST;
    }
    return sum <= budget;
  };
  const runs: { offset: number; limit: number }[] = [];
  for (const pos of positions) {
    const run = runs.at(-1);
    if (
      run !== undefined &&
      pos - run.offset < max &&
      gapFits(run.offset + run.limit - 1, pos)
    ) {
      run.limit = pos - run.offset + 1;
    } else {
      runs.push({ limit: 1, offset: pos });
    }
  }
  return runs;
};

/** Positions a run covers. */
export const runPositions = (offset: number, limit: number) =>
  Array.from({ length: limit }, (_, i) => offset + i);

/** Requests `needed` still lacks: pages whose held hash is not the
 *  summary's, and runs over rows held neither at `rev` nor in a current
 *  page. `skipPage` and `skipRow` name what is in flight, failed or known
 *  stale. */
export const missingFetches = ({
  bodyLen,
  cache,
  hashes,
  needed,
  rev,
  size = PAGE_SIZE,
  skipPage,
  skipRow,
}: {
  readonly bodyLen: (pos: number) => number;
  readonly cache: BodyCache;
  /** `summary.history.pages`. */
  readonly hashes: readonly string[];
  readonly needed: Needed;
  /** `summary.history.rev`. */
  readonly rev: string;
  readonly size?: number;
  readonly skipPage: (page: number) => boolean;
  readonly skipRow: (pos: number) => boolean;
}): BodyFetch[] => {
  const current = (page: number) =>
    hashes[page] !== undefined && cache.pages.get(page)?.hash === hashes[page];
  const pages = needed.pages.flatMap((page): BodyFetch[] => {
    const hash = hashes[page];
    return hash === undefined || current(page) || skipPage(page)
      ? []
      : [{ hash, kind: "page", page }];
  });
  const rows = needed.rows.filter(
    (pos) =>
      cache.rows.get(pos)?.rev !== rev &&
      !current(pageOf(pos, size)) &&
      !skipRow(pos)
  );
  const runs = runsOf(rows, { bodyLen }).map(
    (r): BodyFetch => ({ ...r, kind: "run", rev })
  );
  return [...pages, ...runs];
};

/** `cache` with the answer to `f` stored. */
export const storeBodies = (
  cache: BodyCache,
  f: BodyFetch,
  page: Pick<HistoryPageMeta, "hash" | "rev"> & {
    readonly events: readonly TimelineEvent[];
  },
  size = PAGE_SIZE
): BodyCache => {
  if (f.kind === "run") {
    const rows = new Map(cache.rows);
    page.events.forEach((event, i) => {
      rows.set(f.offset + i, { event, rev: page.rev });
    });
    return { pages: cache.pages, rows };
  }
  const from = f.page * size;
  const inPage = (pos: number) => pos >= from && pos < from + size;
  return {
    pages: new Map(cache.pages).set(f.page, {
      events: page.events,
      hash: page.hash,
    }),
    rows: [...cache.rows.keys()].some(inPage)
      ? new Map([...cache.rows].filter(([pos]) => !inPage(pos)))
      : cache.rows,
  };
};

/** `cache` without bodies past the session's `pageCount` pages, or the same
 *  object when it has none. */
export const trimBodies = (
  cache: BodyCache,
  pageCount: number,
  size = PAGE_SIZE
): BodyCache => {
  const end = pageCount * size;
  const pagesOut = [...cache.pages.keys()].some((p) => p >= pageCount);
  const rowsOut = [...cache.rows.keys()].some((p) => p >= end);
  return pagesOut || rowsOut
    ? {
        pages: new Map([...cache.pages].filter(([p]) => p < pageCount)),
        rows: new Map([...cache.rows].filter(([p]) => p < end)),
      }
    : cache;
};

/** The whole event at `pos` when it is held, else null. */
export const mergeRow = (
  pos: number,
  cache: BodyCache,
  size = PAGE_SIZE
): TimelineEvent | null =>
  cache.rows.get(pos)?.event ??
  cache.pages.get(pageOf(pos, size))?.events[pos % size] ??
  null;

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
