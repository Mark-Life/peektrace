/** Tiny hash router — three top-level sections, zero routing deps.
 *
 * Hash routing keeps the static build trivially deep-linkable under
 * `peephole serve` (no server rewrites needed beyond the existing SPA
 * fallback). A section is the part after `#/`, e.g. `#/capabilities`.
 */
import { useSyncExternalStore } from "react";

/** The three inspector sections, in nav order. */
export const ROUTES = ["sessions", "memory", "capabilities"] as const;

/** A routable section id. */
export type RouteId = (typeof ROUTES)[number];

/** Default section when the hash is empty or unrecognized. */
const DEFAULT_ROUTE: RouteId = "sessions";

/** Strip a leading `#` and optional `/` from the hash. */
const HASH_PREFIX = /^#\/?/;

/** Parse the current `location.hash` into a known `RouteId`. */
const parseHash = (): RouteId => {
  const raw = window.location.hash.replace(HASH_PREFIX, "");
  return (ROUTES as readonly string[]).includes(raw)
    ? (raw as RouteId)
    : DEFAULT_ROUTE;
};

/** Subscribe to `hashchange` for `useSyncExternalStore`. */
const subscribe = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};

/** Navigate to a section by updating the hash. */
export const navigate = (route: RouteId) => {
  window.location.hash = `#/${route}`;
};

/** Reactively read the current section from the URL hash. */
export const useRoute = (): RouteId =>
  useSyncExternalStore(subscribe, parseHash, () => DEFAULT_ROUTE);
