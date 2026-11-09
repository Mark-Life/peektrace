/** Keyboard reach for the session rail: the rows are one Tab stop, arrows and
 * j / k move between them, `/` jumps to search and ↓ leaves it for the rows. */
import {
  type FocusEvent,
  type KeyboardEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import { HISTORY_SEARCH_ID } from "../../lib/landmarks";
import { focusAfterSelect, rovingStop, stepIndex } from "../../lib/roving";
import { keyOf, shortcutAllowed, useShortcut } from "../../lib/shortcuts";

const rowIdOf = (target: EventTarget | null) =>
  target instanceof Element
    ? (target.closest<HTMLElement>("[data-session-id]")?.dataset.sessionId ??
      null)
    : null;

const ROW_STOP = '[data-session-id][tabindex="0"]';

const focusEl = (el: HTMLElement | null | undefined) => {
  el?.focus({ preventScroll: true });
  el?.scrollIntoView({ block: "nearest" });
};

const rowEl = (list: HTMLElement | null, id: string) =>
  list?.querySelector<HTMLElement>(`[data-session-id="${CSS.escape(id)}"]`);

/** Roving focus over the rows. `ids` in display order; `selectedId` is the
 *  open session. Lives in the rows' own component, so a focus move does not
 *  re-render the filters above them. */
export const useListRoving = ({
  ids,
  selectedId,
}: {
  readonly ids: readonly string[];
  readonly selectedId: string | null;
}) => {
  const group = useRef<HTMLUListElement>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  // Opening a session from anywhere makes it the stop; closing one keeps it.
  const [seenSelected, setSeenSelected] = useState(selectedId);
  if (seenSelected !== selectedId) {
    setSeenSelected(selectedId);
    setFocusedId(
      focusAfterSelect({ previous: seenSelected, selected: selectedId })
    );
  }
  const stop = rovingStop({ focusedId, ids, selectedId });
  /** Focus sits on a row, or sat on one that a filter or reload removed. */
  const inside = useRef(false);
  const lastIndex = useRef(0);

  const focusRow = (id: string) => {
    setFocusedId(id);
    focusEl(rowEl(group.current, id));
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
      setFocusedId(next);
      focusEl(rowEl(group.current, next));
    }
  }, [focusedId, ids]);

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

  return {
    group: { onBlur, onFocus, onKeyDown, ref: group },
    stop,
  };
};

/** The search box: `/` focuses it from anywhere, ↓ moves to the rows' Tab
 *  stop inside `root`. */
export const useListSearch = () => {
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);

  const onSearchKeyDown = (ev: KeyboardEvent) => {
    const row = root.current?.querySelector<HTMLElement>(ROW_STOP);
    if (ev.key === "ArrowDown" && row) {
      ev.preventDefault();
      focusEl(row);
    }
  };

  useShortcut("search", () => {
    // The rail is hidden while a session is open on a narrow screen.
    const own = search.current;
    const target =
      own && own.offsetParent !== null
        ? own
        : document.getElementById(HISTORY_SEARCH_ID);
    target?.focus();
  });

  return { onSearchKeyDown, root, search };
};
