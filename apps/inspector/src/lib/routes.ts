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

/** The three inspector sections, in nav order. */
export const ROUTES = ["sessions", "memory", "capabilities"] as const;

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

/** Navigate to a top-level section (pushes; clears any session selection). */
export const navigate = (section: RouteId) => {
  window.location.hash = buildHash(section);
};

/** Open a session in the sessions section — pushes a shareable history entry. */
export const openSession = (id: string) => {
  window.location.hash = buildHash("sessions", new URLSearchParams({ id }));
};

/** Clear the selected session without adding a history entry (in-app Back). */
export const closeSession = () => {
  replaceHash(buildHash("sessions"));
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

/** Set (or, when `value` is null, clear) one query param on the current hash,
 *  preserving the other params, without adding a history entry. */
export const setHashParam = (key: string, value: string | null) => {
  const loc = parseHash();
  const params = new URLSearchParams(loc.params);
  if (value === null) {
    params.delete(key);
  } else {
    params.set(key, value);
  }
  replaceHash(buildHash(loc.section, params));
};
