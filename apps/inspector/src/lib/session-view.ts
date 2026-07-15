/** Persisted, per-session inspector view state.
 *
 * One `localStorage` key per session id holds the open Collapsible ids, the
 * history search/type filters, and the redaction toggle, so reopening a session
 * (or reloading) restores exactly how it was left. Kept out of the URL to keep
 * deep links clean.
 */
import { useCallback, useMemo } from "react";
import { useLocalStorage } from "./use-local-storage";

/** Persisted, per-session inspector view state. */
export interface SessionViewState {
  /** Open Collapsible ids (transcript events + subagent cards). */
  readonly expanded: readonly string[];
  /** History type-filter value (`"all"` | event kind). */
  readonly kind: string;
  /** History search box value. */
  readonly query: string;
  /** Redaction toggle (`true` = redacted). */
  readonly redacted: boolean;
}

/** Fresh view: nothing expanded, no filters, redacted on. */
export const DEFAULT_SESSION_VIEW: SessionViewState = {
  expanded: [],
  query: "",
  kind: "all",
  redacted: true,
};

/** `localStorage` key for one session's view state. */
export const sessionViewKey = (id: string) => `peektrace:sessionView:${id}`;

/** Stable Collapsible id for a transcript event (position-based so it survives
 *  filter and redaction changes — same event order/count, only bodies differ). */
export const eventCollapseId = (pos: number) => `event:${pos}`;

/** Stable Collapsible id for a subagent card. */
export const subagentCollapseId = (id: string) => `subagent:${id}`;

/** Coerce a parsed-but-untrusted stored value into a valid `SessionViewState`,
 *  filling any missing/wrong-typed field from the default. Guards against stale
 *  schemas and hand-edited storage so the detail view never sees a bad shape. */
const normalizeView = (parsed: unknown): SessionViewState => {
  if (typeof parsed !== "object" || parsed === null) {
    return DEFAULT_SESSION_VIEW;
  }
  const raw = parsed as Partial<Record<keyof SessionViewState, unknown>>;
  return {
    expanded: Array.isArray(raw.expanded)
      ? raw.expanded.filter((x): x is string => typeof x === "string")
      : DEFAULT_SESSION_VIEW.expanded,
    query:
      typeof raw.query === "string" ? raw.query : DEFAULT_SESSION_VIEW.query,
    kind: typeof raw.kind === "string" ? raw.kind : DEFAULT_SESSION_VIEW.kind,
    redacted:
      typeof raw.redacted === "boolean"
        ? raw.redacted
        : DEFAULT_SESSION_VIEW.redacted,
  };
};

/** Typed accessor over one session's persisted view. */
export interface SessionView {
  readonly isExpanded: (id: string) => boolean;
  readonly setExpanded: (ids: readonly string[]) => void;
  readonly setKind: (k: string) => void;
  readonly setQuery: (q: string) => void;
  readonly setRedacted: (r: boolean) => void;
  readonly state: SessionViewState;
  readonly toggleExpanded: (id: string, open: boolean) => void;
}

/** Load + persist the view state for one session id. */
export const useSessionView = (id: string): SessionView => {
  const [state, setState] = useLocalStorage(
    sessionViewKey(id),
    DEFAULT_SESSION_VIEW,
    normalizeView
  );

  const setQuery = useCallback(
    (query: string) => setState((prev) => ({ ...prev, query })),
    [setState]
  );
  const setKind = useCallback(
    (kind: string) => setState((prev) => ({ ...prev, kind })),
    [setState]
  );
  const setRedacted = useCallback(
    (redacted: boolean) => setState((prev) => ({ ...prev, redacted })),
    [setState]
  );

  const expandedSet = useMemo(() => new Set(state.expanded), [state.expanded]);
  const isExpanded = useCallback(
    (collapseId: string) => expandedSet.has(collapseId),
    [expandedSet]
  );
  const setExpanded = useCallback(
    (ids: readonly string[]) =>
      setState((prev) => ({ ...prev, expanded: [...ids] })),
    [setState]
  );
  const toggleExpanded = useCallback(
    (collapseId: string, open: boolean) =>
      setState((prev) => {
        const has = prev.expanded.includes(collapseId);
        if (open === has) {
          return prev;
        }
        return {
          ...prev,
          expanded: open
            ? [...prev.expanded, collapseId]
            : prev.expanded.filter((x) => x !== collapseId),
        };
      }),
    [setState]
  );

  return {
    state,
    setQuery,
    setKind,
    setRedacted,
    isExpanded,
    setExpanded,
    toggleExpanded,
  };
};
