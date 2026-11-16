/** The session detail's data, in parts: a small summary first, then the growth
 *  series and the body-free history index, then event bodies as rows are
 *  drawn, plus server-side history search.
 *
 * The summary refetches on every settled sessions version bump, as the whole
 * analysis did. It carries a hash per part, so a live append refetches only
 * the parts that changed: usually the index and the last page.
 */
import {
  Atom,
  Result,
  useAtomRefresh,
  useAtomValue,
} from "@effect-atom/atom-react";
import { Effect } from "effect";
import { useEffect, useMemo, useRef, useState } from "react";
import { apiGet, apiPath, type Query } from "./api";
import type {
  GrowthSeries,
  HistoryIndex,
  HistoryPage,
  HistorySearchResult,
  SessionSummary,
} from "./api-types.gen";
import {
  type BodyCache,
  type BodyFetch,
  EMPTY_BODIES,
  missingFetches,
  type Needed,
  PAGE_SIZE,
  runPositions,
  storeBodies,
  trimBodies,
} from "./history-pages";
import { settledSessionsVersionAtom, useOnVersionBump } from "./watch-atoms";

/** One open session view: which transcript, and whether secrets are masked. */
export interface SessionKey {
  readonly id: string;
  /** Set when `id` is a subagent transcript, resolved through this session. */
  readonly parent: string | null;
  readonly redact: boolean;
}

/** Body requests in flight at once. */
const MAX_IN_FLIGHT = 4;
/** Wait before a failed body or search request is asked again. */
const RETRY_MS = 2000;

const queryOf = (k: SessionKey): Query => ({
  parent: k.parent ?? undefined,
  redact: k.redact ? undefined : "false",
});

const route = (k: SessionKey, ...rest: readonly string[]) =>
  apiPath("sessions", k.id, ...rest);

/** The summary atom, refreshed on every settled sessions bump. */
export const useSessionSummary = (k: SessionKey) => {
  const { id, parent, redact } = k;
  const atom = useMemo(
    () =>
      Atom.make(
        apiGet<SessionSummary>(route({ id, parent, redact }, "summary"), {
          ...queryOf({ id, parent, redact }),
        })
      ),
    [id, parent, redact]
  );
  const refresh = useAtomRefresh(atom);
  useOnVersionBump(useAtomValue(settledSessionsVersionAtom), refresh);
  return { refresh, result: useAtomValue(atom) };
};

/** A part with its own hash (`/growth`, `/history/index`), fetched beside
 *  the summary and refetched once each time the summary names a hash other
 *  than the one held. A failed part is asked again once per new summary.
 *  `s` is null until the summary is here. */
const usePart = <A extends { readonly hash: string }>(
  k: SessionKey,
  part: readonly string[],
  s: SessionSummary | null,
  want: string | null
) => {
  const { id, parent, redact } = k;
  const path = part.join("/");
  const atom = useMemo(
    () =>
      Atom.make(
        apiGet<A>(route({ id, parent, redact }, ...path.split("/")), {
          ...queryOf({ id, parent, redact }),
        })
      ),
    [id, parent, redact, path]
  );
  const result = useAtomValue(atom);
  const refresh = useAtomRefresh(atom);
  const held = Result.isSuccess(result) ? result.value.hash : null;
  const failed = Result.isFailure(result) && !result.waiting;
  const asked = useRef(want);
  const retried = useRef<SessionSummary | null>(null);
  useEffect(() => {
    if (s === null || want === null) {
      return;
    }
    if (failed) {
      if (retried.current !== s) {
        retried.current = s;
        refresh();
      }
      return;
    }
    if (held === null || held === want || asked.current === want) {
      return;
    }
    asked.current = want;
    refresh();
  }, [failed, held, s, want, refresh]);
  return { refresh, result };
};

export const useGrowth = (k: SessionKey, s: SessionSummary | null) =>
  usePart<GrowthSeries>(k, ["growth"], s, s?.history.growth ?? null);

export const useHistoryIndex = (k: SessionKey, s: SessionSummary | null) =>
  usePart<HistoryIndex>(k, ["history", "index"], s, s?.history.index ?? null);

const keyText = (k: SessionKey) => `${k.id}\n${k.parent ?? ""}\n${k.redact}`;

const pageKey = (page: number) => `p${page}`;
const rowKey = (pos: number) => `@${pos}`;

/** Each failed or stale-marked unit of a fetch: its page, or every row. */
const unitsOf = (f: BodyFetch) =>
  f.kind === "page"
    ? [{ expect: f.hash, key: pageKey(f.page) }]
    : runPositions(f.offset, f.limit).map((pos) => ({
        expect: f.rev,
        key: rowKey(pos),
      }));

type InFlight = Map<
  string,
  { readonly c: AbortController; readonly f: BodyFetch }
>;

/** Aborts and forgets requests `needed` no longer covers. */
const abortUnwanted = (inflight: InFlight, needed: Needed) => {
  const pages = new Set(needed.pages);
  const rows = new Set(needed.rows);
  for (const [fk, { c, f }] of inflight) {
    const wanted =
      f.kind === "page"
        ? pages.has(f.page)
        : runPositions(f.offset, f.limit).some((p) => rows.has(p));
    if (!wanted) {
      c.abort();
      inflight.delete(fk);
    }
  }
};

const boundsOf = (f: BodyFetch) =>
  f.kind === "page"
    ? { limit: PAGE_SIZE, offset: f.page * PAGE_SIZE }
    : { limit: f.limit, offset: f.offset };

/** A page answers the summary when its hash is the summary's; a run, when
 *  it came at the summary's rev. */
const answersSummary = (
  f: BodyFetch,
  page: HistoryPage,
  now: { readonly hashes: readonly string[]; readonly rev: string }
) =>
  f.kind === "page" ? page.hash === now.hashes[f.page] : page.rev === now.rev;

/** Event bodies for the drawn rows: at most `MAX_IN_FLIGHT` requests at
 *  once, and one no longer needed is aborted. A held body stays on screen
 *  while a changed one loads. An answer that does not match the summary (a
 *  page's hash, a run's rev) raced a live write: it is dropped and
 *  `onStale` asks for a new summary. A failed request is asked again after
 *  `RETRY_MS`. */
export const useBodyPages = ({
  bodyLen,
  hashes,
  k,
  needed,
  onStale,
  rev,
}: {
  /** Body length of the event at a position, from the index. */
  readonly bodyLen: (pos: number) => number;
  /** `summary.history.pages`. */
  readonly hashes: readonly string[];
  readonly k: SessionKey;
  readonly needed: Needed;
  readonly onStale: () => void;
  /** `summary.history.rev`. */
  readonly rev: string;
}): BodyCache => {
  const key = keyText(k);
  const [state, setState] = useState<{
    readonly key: string;
    readonly cache: BodyCache;
  }>({ cache: EMPTY_BODIES, key });
  const cache = state.key === key ? state.cache : EMPTY_BODIES;
  if (state.key !== key) {
    setState({ cache, key });
  }
  const inflight = useRef<InFlight>(new Map());
  /** `unit:expected` keys whose answer did not match the summary. */
  const stale = useRef(new Set<string>());
  /** Units whose last request failed, until their retry is due. */
  const failed = useRef(new Set<string>());
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  const latest = useRef({ hashes, k, onStale, rev });
  latest.current = { hashes, k, onStale, rev };
  const [tick, setTick] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: another session or redaction aborts the requests of the last one
  useEffect(
    () => () => {
      for (const { c } of inflight.current.values()) {
        c.abort();
      }
      inflight.current.clear();
      for (const t of timers.current) {
        clearTimeout(t);
      }
      timers.current.clear();
      failed.current.clear();
    },
    [key]
  );

  useEffect(() => {
    if (tick < 0) {
      return;
    }
    if (trimBodies(cache, hashes.length) !== cache) {
      setState((s) => ({
        cache: trimBodies(s.cache, hashes.length),
        key: s.key,
      }));
    }
    abortUnwanted(inflight.current, needed);
    const busy = new Set(
      [...inflight.current.values()].flatMap(({ f }) =>
        unitsOf(f).map((u) => u.key)
      )
    );
    const skip = (unit: string, expect: string | undefined) =>
      busy.has(unit) ||
      failed.current.has(unit) ||
      stale.current.has(`${unit}:${expect}`);
    const todo = missingFetches({
      bodyLen,
      cache,
      hashes,
      needed,
      rev,
      skipPage: (p) => skip(pageKey(p), hashes[p]),
      skipRow: (pos) => skip(rowKey(pos), rev),
    });
    for (const f of todo) {
      if (inflight.current.size >= MAX_IN_FLIGHT) {
        break;
      }
      const fk =
        f.kind === "page" ? pageKey(f.page) : `r${f.offset}+${f.limit}`;
      const c = new AbortController();
      inflight.current.set(fk, { c, f });
      const done = () => {
        if (inflight.current.get(fk)?.c === c) {
          inflight.current.delete(fk);
          setTick((t) => t + 1);
        }
      };
      const ask = apiGet<HistoryPage>(route(k, "history"), {
        ...queryOf(k),
        ...boundsOf(f),
      });
      Effect.runPromise(ask, { signal: c.signal }).then(
        (page) => {
          if (inflight.current.get(fk)?.c !== c) {
            return;
          }
          const now = latest.current;
          if (answersSummary(f, page, now)) {
            setState((s) =>
              s.key === key ? { cache: storeBodies(s.cache, f, page), key } : s
            );
          } else {
            for (const u of unitsOf(f)) {
              stale.current.add(`${u.key}:${u.expect}`);
            }
            now.onStale();
          }
          done();
        },
        () => {
          if (!c.signal.aborted) {
            const units = unitsOf(f).map((u) => u.key);
            for (const u of units) {
              failed.current.add(u);
            }
            const t = setTimeout(() => {
              timers.current.delete(t);
              for (const u of units) {
                failed.current.delete(u);
              }
              setTick((n) => n + 1);
            }, RETRY_MS);
            timers.current.add(t);
          }
          done();
        }
      );
    }
  }, [needed, hashes, rev, bodyLen, cache, k, key, tick]);

  return cache;
};

/** What the history filters by while searching: the positions matched for
 *  `query`, which may trail the typed query while a request runs. */
export interface SearchHits {
  readonly hits: ReadonlySet<number> | undefined;
  readonly pending: boolean;
  /** The query `hits` answer; "" before the first answer. */
  readonly query: string;
}

/** A search answer and what it was asked for. */
export interface SearchAnswer {
  readonly hits: ReadonlySet<number>;
  readonly query: string;
  /** `keyText` of the session searched. */
  readonly session: string;
}

/** What the history shows for `query` given the latest answer. Only a new
 *  query or session is pending: a new revision keeps the last hits shown,
 *  so live appends do not dim the transcript while it searches again. */
export const searchHitsOf = ({
  answer,
  query,
  session,
}: {
  readonly answer: SearchAnswer | null;
  readonly query: string;
  readonly session: string;
}): SearchHits => {
  if (query === "") {
    return { hits: undefined, pending: false, query: "" };
  }
  return {
    hits: answer?.hits,
    pending: answer?.session !== session || answer.query !== query,
    query: answer?.query ?? "",
  };
};

/** Server-side history search over title, preview and body, case folded.
 *  A newer query aborts the older request; answers are kept per summary
 *  revision, so stepping back to an earlier query is instant. A failed
 *  request is asked again after `RETRY_MS`. */
export const useHistorySearch = ({
  k,
  query,
  rev,
}: {
  readonly k: SessionKey;
  readonly query: string;
  /** `summary.history.rev`: a new revision searches again. */
  readonly rev: string;
}): SearchHits => {
  const [answer, setAnswer] = useState<SearchAnswer | null>(null);
  const cache = useRef(new Map<string, ReadonlySet<number>>());
  const session = keyText(k);
  const key = `${session}\n${rev}`;
  const want = `${key}\n${query}`;
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (query === "" || attempt < 0) {
      return;
    }
    const held = cache.current.get(want);
    if (held) {
      setAnswer({ hits: held, query, session });
      return;
    }
    const c = new AbortController();
    let retry: ReturnType<typeof setTimeout> | undefined;
    const ask = apiGet<HistorySearchResult>(route(k, "history", "search"), {
      ...queryOf(k),
      q: query,
    });
    Effect.runPromise(ask, { signal: c.signal }).then(
      (r) => {
        const hits = new Set(r.positions);
        cache.current.set(want, hits);
        setAnswer({ hits, query, session });
      },
      () => {
        // The last answer stays meanwhile.
        if (!c.signal.aborted) {
          retry = setTimeout(() => setAttempt((n) => n + 1), RETRY_MS);
        }
      }
    );
    return () => {
      c.abort();
      clearTimeout(retry);
    };
  }, [k, query, session, want, attempt]);
  return searchHitsOf({ answer, query, session });
};
