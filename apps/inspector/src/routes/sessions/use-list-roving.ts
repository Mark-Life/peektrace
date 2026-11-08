/** Keyboard reach for the session rail: the rows are one Tab stop, arrows and
 * j / k move between them, `/` jumps to search. */
import {
  type FocusEvent,
  type KeyboardEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import { rovingStop, stepIndex } from "../../lib/roving";
import { keyOf, shortcutAllowed, useShortcut } from "../../lib/shortcuts";

/** The search box on screen when the rail is hidden (a session is open on a
 *  narrow screen). */
const HISTORY_SEARCH_ID = "history-search";

const rowIdOf = (target: EventTarget | null) =>
  target instanceof Element
    ? (target.closest<HTMLElement>("[data-session-id]")?.dataset.sessionId ??
      null)
    : null;

/** `ids` in display order; `selectedId` is the open session. */
export const useListRoving = ({
  ids,
  selectedId,
}: {
  readonly ids: readonly string[];
  readonly selectedId: string | null;
}) => {
  const group = useRef<HTMLUListElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  // Opening a session from anywhere makes it the stop again.
  const [seenSelected, setSeenSelected] = useState(selectedId);
  if (seenSelected !== selectedId) {
    setSeenSelected(selectedId);
    setFocusedId(null);
  }
  const stop = rovingStop({ focusedId, ids, selectedId });
  /** Focus sits on a row, or sat on one that a filter or reload removed. */
  const inside = useRef(false);
  const lastIndex = useRef(0);

  const focusRow = (id: string) => {
    setFocusedId(id);
    const el = group.current?.querySelector<HTMLElement>(
      `[data-session-id="${CSS.escape(id)}"]`
    );
    el?.focus({ preventScroll: true });
    el?.scrollIntoView({ block: "nearest" });
  };

  useEffect(() => {
    if (focusedId === null) {
      return;
    }
    const index = ids.indexOf(focusedId);
    if (index >= 0) {
      lastIndex.current = index;
      return;
    }
    const active = document.activeElement;
    const lost = active === null || active === document.body;
    const next = ids[Math.min(lastIndex.current, ids.length - 1)];
    if (inside.current && lost && next !== undefined) {
      focusRow(next);
    }
  });

  const onKeyDown = (ev: KeyboardEvent) => {
    const id = rowIdOf(ev.target);
    if (id === null || !shortcutAllowed(ev.nativeEvent)) {
      return;
    }
    const next = stepIndex({
      count: ids.length,
      index: ids.indexOf(id),
      key: keyOf(ev),
    });
    const nextId = next === null ? undefined : ids[next];
    if (nextId === undefined) {
      return;
    }
    ev.preventDefault();
    focusRow(nextId);
  };

  const onFocus = (ev: FocusEvent) => {
    const id = rowIdOf(ev.target);
    if (id !== null) {
      inside.current = true;
      setFocusedId(id);
    }
  };

  const onBlur = (ev: FocusEvent) => {
    // A row removed while focused is still "inside"; a click elsewhere is not.
    if (ev.target.isConnected) {
      inside.current = false;
    }
  };

  const onSearchKeyDown = (ev: KeyboardEvent) => {
    if (ev.key === "ArrowDown" && stop !== null) {
      ev.preventDefault();
      focusRow(stop);
    }
  };

  useShortcut("search", () => {
    const own = search.current;
    const target =
      own && own.offsetParent !== null
        ? own
        : document.getElementById(HISTORY_SEARCH_ID);
    target?.focus();
  });

  return {
    group: { onBlur, onFocus, onKeyDown, ref: group },
    onSearchKeyDown,
    search,
    stop,
  };
};
