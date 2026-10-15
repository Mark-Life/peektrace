/** History view prefs shared by every session.
 *
 * Layout, sort, type filter and search live under one `localStorage` key, so
 * opening another session keeps the same reading of its history. Redaction and
 * open rows stay per session (see `session-view.ts`): revealing secrets in one
 * transcript must never reveal them in the next.
 *
 * A module-level store backs the hook, so every component reading the prefs
 * sees one value and re-renders together on same-tab writes; the `storage`
 * event keeps other tabs in step.
 */
import { useCallback, useSyncExternalStore } from "react";
import { setHashParam, useHashParam } from "./routes";
import {
  DEFAULT_TRANSCRIPT_MODE,
  resolveTranscriptMode,
  type TranscriptMode,
  transcriptModeOf,
} from "./transcript-mode";

/** History row order: transcript order, or largest events first. */
export type HistorySort = "order" | "size";

/** Persisted history prefs, shared across sessions. */
export interface HistoryPrefs {
  /** History type-filter value (`"all"` | event kind). */
  readonly kind: string;
  /** Stored transcript layout (the hash `view` param can override it). */
  readonly mode: TranscriptMode;
  /** History search box value. */
  readonly query: string;
  /** History row order. */
  readonly sort: HistorySort;
}

/** Fresh prefs: chat layout, no filters, transcript order. */
export const DEFAULT_HISTORY_PREFS: HistoryPrefs = {
  mode: DEFAULT_TRANSCRIPT_MODE,
  sort: "order",
  kind: "all",
  query: "",
};

/** `localStorage` key for the shared history prefs. */
export const HISTORY_PREFS_KEY = "peektrace:historyPrefs";

/** Coerce an untrusted stored value into valid prefs, field by field. */
export const normalizeHistoryPrefs = (parsed: unknown): HistoryPrefs => {
  if (typeof parsed !== "object" || parsed === null) {
    return DEFAULT_HISTORY_PREFS;
  }
  const raw = parsed as Partial<Record<keyof HistoryPrefs, unknown>>;
  return {
    mode: transcriptModeOf(raw.mode) ?? DEFAULT_HISTORY_PREFS.mode,
    sort: raw.sort === "size" ? "size" : DEFAULT_HISTORY_PREFS.sort,
    kind: typeof raw.kind === "string" ? raw.kind : DEFAULT_HISTORY_PREFS.kind,
    query:
      typeof raw.query === "string" ? raw.query : DEFAULT_HISTORY_PREFS.query,
  };
};

const hasStorage = () => typeof window !== "undefined";

const readPrefs = () => {
  if (!hasStorage()) {
    return DEFAULT_HISTORY_PREFS;
  }
  try {
    const raw = window.localStorage.getItem(HISTORY_PREFS_KEY);
    return raw === null
      ? DEFAULT_HISTORY_PREFS
      : normalizeHistoryPrefs(JSON.parse(raw));
  } catch {
    return DEFAULT_HISTORY_PREFS;
  }
};

let current: HistoryPrefs | undefined;
const listeners = new Set<() => void>();

const notify = () => {
  for (const l of listeners) {
    l();
  }
};

const onStorage = (e: StorageEvent) => {
  if (e.key === HISTORY_PREFS_KEY || e.key === null) {
    current = readPrefs();
    notify();
  }
};

/** Current prefs; stable identity until the next write. */
export const getHistoryPrefs = () => {
  current ??= readPrefs();
  return current;
};

/** Merge a patch into the prefs, persist it, and notify every reader. */
export const updateHistoryPrefs = (patch: Partial<HistoryPrefs>) => {
  const prev = getHistoryPrefs();
  const next = { ...prev, ...patch };
  if (
    (Object.keys(next) as (keyof HistoryPrefs)[]).every(
      (k) => next[k] === prev[k]
    )
  ) {
    return;
  }
  current = next;
  if (hasStorage()) {
    try {
      window.localStorage.setItem(HISTORY_PREFS_KEY, JSON.stringify(next));
    } catch {
      // Private mode / quota: keep the in-memory value, unpersisted.
    }
  }
  notify();
};

/** Subscribe to prefs changes (same tab and other tabs). */
export const subscribeHistoryPrefs = (listener: () => void) => {
  if (listeners.size === 0 && hasStorage()) {
    window.addEventListener("storage", onStorage);
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && hasStorage()) {
      window.removeEventListener("storage", onStorage);
    }
  };
};

/** Shared history prefs plus the effective layout (hash override applied). */
export const useHistoryPrefs = () => {
  const prefs = useSyncExternalStore(
    subscribeHistoryPrefs,
    getHistoryPrefs,
    () => DEFAULT_HISTORY_PREFS
  );
  const mode = resolveTranscriptMode({
    hash: useHashParam("view"),
    stored: prefs.mode,
  });

  const setQuery = useCallback(
    (query: string) => updateHistoryPrefs({ query }),
    []
  );
  const setKind = useCallback(
    (kind: string) => updateHistoryPrefs({ kind }),
    []
  );
  const setSort = useCallback(
    (sort: HistorySort) => updateHistoryPrefs({ sort }),
    []
  );
  // The hash mirrors the choice so the URL stays a faithful, shareable link.
  const setMode = useCallback((next: TranscriptMode) => {
    updateHistoryPrefs({ mode: next });
    setHashParam("view", next);
  }, []);

  return { ...prefs, mode, setQuery, setKind, setSort, setMode };
};
