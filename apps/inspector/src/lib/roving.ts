/** Roving tabindex for a list: one row is the Tab stop, the arrow keys (and
 * j / k) move focus between rows. */
import { type ShortcutId, shortcutOf } from "./shortcuts";

/** The id that gets `tabIndex=0`: the focused row while it is still listed,
 *  else the selected row, else the first. */
export const rovingStop = ({
  focusedId,
  ids,
  selectedId,
}: {
  readonly focusedId: string | null;
  readonly ids: readonly string[];
  readonly selectedId: string | null;
}) => {
  if (focusedId !== null && ids.includes(focusedId)) {
    return focusedId;
  }
  if (selectedId !== null && ids.includes(selectedId)) {
    return selectedId;
  }
  return ids[0] ?? null;
};

/** The focused row after the open session changes from `previous` to
 *  `selected`: opening one hands the stop to it, closing one keeps the closed
 *  row as the stop so the reader does not lose their place. */
export const focusAfterSelect = ({
  previous,
  selected,
}: {
  readonly previous: string | null;
  readonly selected: string | null;
}) => (selected === null ? previous : null);

const has = (id: ShortcutId, key: string) => shortcutOf(id).keys.includes(key);

/** The row `key` moves focus to from `index`, clamped to the list, or null
 *  when the key is not a list key or the list is empty. */
export const stepIndex = ({
  count,
  index,
  key,
}: {
  readonly count: number;
  readonly index: number;
  readonly key: string;
}) => {
  if (count === 0) {
    return null;
  }
  const last = count - 1;
  if (has("listDown", key)) {
    return Math.min(index + 1, last);
  }
  if (has("listUp", key)) {
    return Math.max(index - 1, 0);
  }
  if (has("listFirst", key)) {
    return 0;
  }
  if (has("listLast", key)) {
    return last;
  }
  return null;
};
