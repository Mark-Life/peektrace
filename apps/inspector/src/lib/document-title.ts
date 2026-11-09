/** Per-route `document.title`, so tabs, history and bookmarks name the page. */
import { useEffect } from "react";

/** The tab title for a page label. */
export const pageTitle = (label: string) => `${label} · Peektrace`;

/** The shell names the route (`base`); a mounted page that knows better (the
 *  open session) sets `override`, which wins until it unmounts. Keeping both
 *  means effect order between shell and page never matters. */
interface TitleParts {
  readonly base: string | null;
  readonly override: string | null;
}

let parts: TitleParts = { base: null, override: null };

const apply = () => {
  const label = parts.override ?? parts.base;
  if (label !== null && typeof document !== "undefined") {
    document.title = pageTitle(label);
  }
};

export const setTitleBase = (base: string | null) => {
  parts = { ...parts, base };
  apply();
};

export const setTitleOverride = (override: string | null) => {
  parts = { ...parts, override };
  apply();
};

/** Name the current route; used by the shell. */
export const useDocumentTitle = (label: string | null) => {
  useEffect(() => setTitleBase(label), [label]);
};

/** Name the page more precisely than its route while mounted; `null` defers
 *  to the route label. */
export const useTitleOverride = (label: string | null) => {
  useEffect(() => {
    setTitleOverride(label);
    return () => setTitleOverride(null);
  }, [label]);
};
