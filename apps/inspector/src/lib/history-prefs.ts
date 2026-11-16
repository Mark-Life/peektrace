/** History view prefs shared by every session.
 *
 * Layout, sort and type filter live under one `localStorage` key, so opening
 * another session keeps the same reading of its history. Redaction and open
 * rows stay per session (see `session-view.ts`): revealing secrets in one
 * transcript must never reveal them in the next. The search query is not a
 * pref: it belongs to one session view and clears when another opens, so a
 * stale query can never silently filter the next transcript.
 *
 * A module-level store backs the hook, so every component reading the prefs
 * sees one value and re-renders together on same-tab writes; the `storage`
 * event keeps other tabs in step.
 */
import { useCallback, useSyncExternalStore } from "react";

import { DEFAULT_HISTORY_SORT, historySortOf, sameSort } from "./history-sort";
import type { HistorySort } from "./history-sort";
import { setHashParam, useHashParam } from "./routes";
import {
  DEFAULT_TRANSCRIPT_MODE,
  resolveTranscriptMode,
  transcriptModeOf,
} from "./transcript-mode";
import type { TranscriptMode } from "./transcript-mode";

/** Persisted history prefs, shared across sessions. */
export interface HistoryPrefs {
  /** History type-filter value (`"all"` | event kind). */
  readonly kind: string;
  /** Stored transcript layout (the hash `view` param can override it). */
  readonly mode: TranscriptMode;
  /** Table row sort (chat always reads in transcript order). */
  readonly sort: HistorySort;
}

/** Fresh prefs: chat layout, no filters, transcript order. */
export const DEFAULT_HISTORY_PREFS: HistoryPrefs = {
  kind: "all",
  mode: DEFAULT_TRANSCRIPT_MODE,
  sort: DEFAULT_HISTORY_SORT,
};

/** `localStorage` key for the shared history prefs. */
export const HISTORY_PREFS_KEY = "peektrace:historyPrefs";

/** The fields a stored value may carry, each still untrusted. */
interface StoredPrefs {
  readonly kind?: unknown;
  readonly mode?: unknown;
  readonly sort?: unknown;
}

const isStoredPrefs = (u: unknown): u is StoredPrefs =>
  typeof u === "object" && u !== null;

const isText = (u: unknown): u is string => typeof u === "string";

/** Coerce an untrusted stored value into valid prefs, field by field. */
// oxlint-disable-next-line anti-slop/no-unknown-parameters -- this function is the parser for the raw `localStorage` payload
export const normalizeHistoryPrefs = (parsed: unknown): HistoryPrefs => {
  if (!isStoredPrefs(parsed)) {
    return DEFAULT_HISTORY_PREFS;
  }

  return {
    kind: isText(parsed.kind) ? parsed.kind : DEFAULT_HISTORY_PREFS.kind,
    mode: transcriptModeOf(parsed.mode) ?? DEFAULT_HISTORY_PREFS.mode,
    sort: historySortOf(parsed.sort),
  };
};

/** A real browser window. Test renderers can leave a bare `window` global behind. */
const hasStorage = () =>
  // oxlint-disable-next-line anti-slop/no-runtime-typeof -- probes the host environment, not input data
  typeof window !== "undefined" &&
  // oxlint-disable-next-line anti-slop/no-runtime-typeof -- probes the host environment, not input data
  typeof window.addEventListener === "function";

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

const PREF_KEYS = [
  "kind",
  "mode",
  "sort",
] as const satisfies readonly (keyof HistoryPrefs)[];

/** Merge a patch into the prefs, persist it, and notify every reader. */
export const updateHistoryPrefs = (patch: Partial<HistoryPrefs>) => {
  const prev = getHistoryPrefs();
  const next = { ...prev, ...patch };

  if (
    PREF_KEYS.every((k) =>
      k === "sort" ? sameSort(next.sort, prev.sort) : next[k] === prev[k]
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

  const setKind = useCallback((kind: string) => {
    updateHistoryPrefs({ kind });
  }, []);

  const setSort = useCallback((sort: HistorySort) => {
    updateHistoryPrefs({ sort });
  }, []);

  // The hash mirrors the choice so the URL stays a faithful, shareable link.
  const setMode = useCallback((next: TranscriptMode) => {
    updateHistoryPrefs({ mode: next });
    setHashParam("view", next);
  }, []);

  return { ...prefs, mode, setKind, setMode, setSort };
};
