/** The session detail's data, in parts: a small summary first, then the growth
 *  series and the body-free history index, then event pages as rows are
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
import { PAGE_SIZE, type PageCache } from "./history-pages";
import { settledSessionsVersionAtom, useOnVersionBump } from "./watch-atoms";

/** One open session view: which transcript, and whether secrets are masked. */
export interface SessionKey {
  readonly id: string;
  /** Set when `id` is a subagent transcript, resolved through this session. */
  readonly parent: string | null;
  readonly redact: boolean;
}

/** Page requests in flight at once. */
const MAX_IN_FLIGHT = 4;
/** Wait before a failed page or search request is asked again. */
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

/** Event pages for the drawn rows: at most `MAX_IN_FLIGHT` requests at once,
 *  and a page no longer needed is aborted. A held page stays on screen while
 *  a changed one loads. A page whose hash is not the summary's raced a live
 *  write: it is dropped and `onStale` asks for a new summary. A failed
 *  request is asked again after `RETRY_MS`. */
export const useBodyPages = ({
  hashes,
  k,
  needed,
  onStale,
}: {
  /** `summary.history.pages`. */
  readonly hashes: readonly string[];
  readonly k: SessionKey;
  /** Page numbers the drawn rows need, sorted. */
  readonly needed: readonly number[];
  readonly onStale: () => void;
}): PageCache => {
  const key = keyText(k);
  const [state, setState] = useState<{
    readonly key: string;
    readonly pages: PageCache;
  }>({ key, pages: new Map() });
  const pages = state.key === key ? state.pages : new Map();
  if (state.key !== key) {
    setState({ key, pages });
  }
  const inflight = useRef(new Map<number, AbortController>());
  /** `page:hash` keys whose answer did not match the summary. */
  const stale = useRef(new Set<string>());
  /** Pages whose last request failed, until their retry is due. */
  const failed = useRef(new Set<number>());
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  const latest = useRef({ hashes, k, onStale });
  latest.current = { hashes, k, onStale };
  const [tick, setTick] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: another session or redaction aborts the requests of the last one
  useEffect(
    () => () => {
      for (const c of inflight.current.values()) {
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
    if ([...pages.keys()].some((p) => p >= hashes.length)) {
      setState((s) => ({
        key: s.key,
        pages: new Map([...s.pages].filter(([p]) => p < hashes.length)),
      }));
    }
    const wanted = new Set(needed);
    for (const [p, c] of inflight.current) {
      if (!wanted.has(p)) {
        c.abort();
        inflight.current.delete(p);
      }
    }
    const missing = needed.filter((p) => {
      const hash = hashes[p];
      return (
        hash !== undefined &&
        pages.get(p)?.hash !== hash &&
        !inflight.current.has(p) &&
        !failed.current.has(p) &&
        !stale.current.has(`${p}:${hash}`)
      );
    });
    for (const p of missing) {
      if (inflight.current.size >= MAX_IN_FLIGHT) {
        break;
      }
      const expected = hashes[p] ?? "";
      const c = new AbortController();
      inflight.current.set(p, c);
      const done = () => {
        if (inflight.current.get(p) === c) {
          inflight.current.delete(p);
          setTick((t) => t + 1);
        }
      };
      const ask = apiGet<HistoryPage>(route(k, "history"), {
        ...queryOf(k),
        limit: PAGE_SIZE,
        offset: p * PAGE_SIZE,
      });
      Effect.runPromise(ask, { signal: c.signal }).then(
        (page) => {
          if (inflight.current.get(p) !== c) {
            return;
          }
          if (page.hash === latest.current.hashes[p]) {
            setState((s) =>
              s.key === key
                ? {
                    key,
                    pages: new Map(s.pages).set(p, {
                      events: page.events,
                      hash: page.hash,
                    }),
                  }
                : s
            );
          } else {
            stale.current.add(`${p}:${expected}`);
            latest.current.onStale();
          }
          done();
        },
        () => {
          if (!c.signal.aborted) {
            failed.current.add(p);
            const t = setTimeout(() => {
              timers.current.delete(t);
              failed.current.delete(p);
              setTick((n) => n + 1);
            }, RETRY_MS);
            timers.current.add(t);
          }
          done();
        }
      );
    }
  }, [needed, hashes, pages, k, key, tick]);

  return pages;
};

/** What the history filters by while searching: the positions matched for
 *  `query`, which may trail the typed query while a request runs. */
export interface SearchHits {
  readonly hits: ReadonlySet<number> | undefined;
  readonly pending: boolean;
  /** The query `hits` answer; "" before the first answer. */
  readonly query: string;
}

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
  const [answer, setAnswer] = useState<{
    readonly hits: ReadonlySet<number>;
    readonly key: string;
    readonly query: string;
  } | null>(null);
  const cache = useRef(new Map<string, ReadonlySet<number>>());
  const key = `${keyText(k)}\n${rev}`;
  const want = `${key}\n${query}`;
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (query === "" || attempt < 0) {
      return;
    }
    const held = cache.current.get(want);
    if (held) {
      setAnswer({ hits: held, key, query });
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
        setAnswer({ hits, key, query });
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
  }, [k, key, query, want, attempt]);
  if (query === "") {
    return { hits: undefined, pending: false, query: "" };
  }
  const fresh = answer !== null && answer.key === key && answer.query === query;
  return {
    hits: answer?.hits,
    pending: !fresh,
    query: answer?.query ?? "",
  };
};
