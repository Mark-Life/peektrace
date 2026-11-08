/** Tiny hash router — three top-level sections + per-section query, zero deps.
 *
 * Hash routing keeps the static build trivially deep-linkable under
 * `peektrace serve` (no server rewrites needed beyond the existing SPA
 * fallback). A section is the part after `#/`, e.g. `#/capabilities`. The
 * sessions section additionally carries an `?id=<sessionId>` query so an open
 * session is a shareable deep link. The hash query is entirely separate from
 * `window.location.search` (where `?titlebar=inset` lives), so host chrome
 * detection is unaffected.
 */
import { useSyncExternalStore } from "react";

/** The inspector sections, in nav order. */
export const ROUTES = [
  "sessions",
  "stats",
  "memory",
  "capabilities",
  "settings",
] as const;

/** A routable section id. */
export type RouteId = (typeof ROUTES)[number];

/** Default section when the hash is empty or unrecognized. */
const DEFAULT_ROUTE: RouteId = "sessions";

/** Leading `#` and optional `/` of a hash, stripped before parsing the body. */
const HASH_PREFIX = /^#\/?/;

/** Parsed hash: which section + its query params. */
export interface HashLocation {
  readonly params: URLSearchParams;
  readonly section: RouteId;
}

/** Module-level cache so repeated `getSnapshot` calls return a stable ref. */
let cache: { readonly raw: string; readonly loc: HashLocation } | null = null;

/** Hash keys that hold the session list filters. */
export const LIST_KEYS = [
  "q",
  "agent",
  "project",
  "branch",
  "model",
  "source",
  "from",
  "to",
] as const;

/** One session list filter key. */
export type ListKey = (typeof LIST_KEYS)[number];

/** Only the list filter keys of `params`, in `LIST_KEYS` order. */
export const listParams = (params: URLSearchParams) => {
  const out = new URLSearchParams();
  for (const key of LIST_KEYS) {
    const value = params.get(key);
    if (value !== null) {
      out.set(key, value);
    }
  }
  return out;
};

/** The list filters of the last sessions hash seen, so the nav can restore them. */
let lastList = new URLSearchParams();

/**
 * Parse `location.hash` into a section + params, memoized by the raw hash
 * string so repeated `getSnapshot` calls return a stable reference (required
 * by `useSyncExternalStore` to avoid an infinite render loop).
 */
const parseHash = (): HashLocation => {
  const raw = window.location.hash;
  if (cache && cache.raw === raw) {
    return cache.loc;
  }
  const body = raw.replace(HASH_PREFIX, "");
  const [head, tail] = body.split("?", 2) as [string, string | undefined];
  const section = (ROUTES as readonly string[]).includes(head)
    ? (head as RouteId)
    : DEFAULT_ROUTE;
  const loc: HashLocation = {
    section,
    params: new URLSearchParams(tail ?? ""),
  };
  cache = { raw, loc };
  if (section === "sessions") {
    lastList = listParams(loc.params);
  }
  return loc;
};

/** Subscribe to `hashchange` for `useSyncExternalStore`. */
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};

/** Build a hash string from a section (+optional params); omits empty query. */
const buildHash = (section: RouteId, params?: URLSearchParams) => {
  const query = params?.toString() ?? "";
  return query.length > 0 ? `#/${section}?${query}` : `#/${section}`;
};

/** Replace the current history entry with `hash` and notify hash subscribers.
 *  `replaceState` fires no `hashchange`, so dispatch one manually. */
const replaceHash = (hash: string) => {
  window.history.replaceState(null, "", hash);
  const event =
    typeof HashChangeEvent === "function"
      ? new HashChangeEvent("hashchange")
      : new Event("hashchange");
  window.dispatchEvent(event);
};

/** Navigate to a top-level section (pushes; clears any session selection).
 *  The sessions section gets back the list filters it last had. */
export const navigate = (section: RouteId) => {
  parseHash();
  window.location.hash = buildHash(
    section,
    section === "sessions" ? lastList : undefined
  );
};

/** The current list filters plus `extra`, for a hash that keeps the rail as is. */
const withList = (extra: Record<string, string>) => {
  const params = listParams(parseHash().params);
  for (const [key, value] of Object.entries(extra)) {
    params.set(key, value);
  }
  return params;
};

/** Open a session in the sessions section — pushes a shareable history entry. */
export const openSession = (id: string) => {
  window.location.hash = buildHash("sessions", withList({ id }));
};

/** A session to open: a nested transcript also names the session it ran under. */
export interface SessionTarget {
  readonly id: string;
  readonly parent: string | null;
}

/** Query for a session target; `parent` is omitted for a top-level session. */
const targetParams = ({ id, parent }: SessionTarget) =>
  new URLSearchParams(parent === null ? { id } : { id, parent });

/** A session opened from a stats row: the detector that sent you there, and
 * the call (its tool-use id) to bring into view, when one is meant. */
type MarkedTarget = SessionTarget & {
  readonly mark: string;
  readonly call?: string | null;
};

/**
 * The hash a stats row opens, carrying the detector id that sent you there so
 * the transcript can highlight the calls behind the number, and `call=<id>`
 * so it scrolls to the one meant. Marks render only in the chat layout, so
 * `view=chat` overrides a stored table preference.
 */
export const markedSessionHash = (target: MarkedTarget) => {
  const params = targetParams(target);
  params.set("mark", target.mark);
  if (target.call !== undefined && target.call !== null) {
    params.set("call", target.call);
  }
  params.set("view", "chat");
  return buildHash("sessions", params);
};

/** Open a session from a stats row, marked by the row's detector. */
export const openSessionMarked = (target: MarkedTarget) => {
  window.location.hash = markedSessionHash(target);
};

/** Open a subagent transcript through the session it ran under. */
export const openSubagent = ({ id, parent }: SessionTarget) => {
  window.location.hash = buildHash(
    "sessions",
    withList(parent === null ? { id } : { id, parent })
  );
};

/** Clear the selected session without adding a history entry (in-app Back).
 *  The list filters stay. */
export const closeSession = () => {
  replaceHash(buildHash("sessions", listParams(parseHash().params)));
};

/** Reactively read the current section from the URL hash. */
export const useRoute = () =>
  useSyncExternalStore(
    subscribe,
    () => parseHash().section,
    () => DEFAULT_ROUTE
  );

/** Reactively read the selected session id from the hash, or null. */
export const useSelectedSessionId = () =>
  useSyncExternalStore(
    subscribe,
    () => parseHash().params.get("id"),
    () => null
  );

/** Reactively read one query param from the current hash, or null. */
export const useHashParam = (key: string) =>
  useSyncExternalStore(
    subscribe,
    () => parseHash().params.get(key),
    () => null
  );

/** One query param from the current hash, read once, or null. */
export const readHashParam = (key: string) => parseHash().params.get(key);

/** Reactively read the list filter keys of the current hash as a query string. */
export const useListQuery = () =>
  useSyncExternalStore(
    subscribe,
    () => listParams(parseHash().params).toString(),
    () => ""
  );

/** Set (or, when a value is null, clear) query params on the current hash in
 *  one write, preserving the other params, without adding a history entry. */
export const setHashParams = (
  updates: Readonly<Record<string, string | null>>
) => {
  const loc = parseHash();
  const params = new URLSearchParams(loc.params);
  for (const [key, value] of Object.entries(updates)) {
    if (value === null) {
      params.delete(key);
    } else {
      params.set(key, value);
    }
  }
  const hash = buildHash(loc.section, params);
  if (hash !== window.location.hash) {
    replaceHash(hash);
  }
};

/** Set (or, when `value` is null, clear) one query param on the current hash,
 *  preserving the other params, without adding a history entry. */
export const setHashParam = (key: string, value: string | null) =>
  setHashParams({ [key]: value });
