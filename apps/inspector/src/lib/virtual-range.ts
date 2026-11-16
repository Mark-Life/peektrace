/** Which items a virtual list mounts, beyond the ones on screen.
 *
 * A focused block that scrolls away must stay mounted, or focus falls back to
 * `<body>` and the keyboard reader loses their place. It is the one item that
 * can sit outside the contiguous on-screen run, and is then drawn on its own.
 */

const LAST = -1;
const BEFORE_LAST = -2;

/** `indexes` (ascending) plus `keep`, still ascending. */
export const keepIndex = ({
  count,
  indexes,
  keep,
}: {
  readonly count: number;
  readonly indexes: readonly number[];
  readonly keep: number | null;
}) => {
  if (keep === null || keep < 0 || keep >= count || indexes.includes(keep)) {
    return [...indexes];
  }

  return [
    ...indexes.filter((i) => i < keep),
    keep,
    ...indexes.filter((i) => i > keep),
  ];
};

/** Split ascending items into the contiguous run and at most one stray item
 *  at either end that `keepIndex` added. */
export const splitPinned = <T extends { readonly index: number }>(
  items: readonly T[]
) => {
  const [first, second] = items;

  if (first && second && second.index !== first.index + 1) {
    return { flow: items.slice(1), pinned: [first] };
  }

  const last = items.at(LAST);
  const beforeLast = items.at(BEFORE_LAST);

  if (last && beforeLast && last.index !== beforeLast.index + 1) {
    return { flow: items.slice(0, -1), pinned: [last] };
  }

  return { flow: [...items], pinned: [] };
};

/** What a parent can do with a mounted virtual list. */
export interface VirtualListHandle {
  /** The drawn element of item `index`, or null when it is not mounted. */
  readonly elementAt: (index: number) => HTMLElement | null;
  readonly scrollToIndex: (index: number, align: "start" | "center") => void;
  /** Where the reader is: the focused item while it is on screen, else the
   *  item at the top of the pane, or -1 when the list starts below the top. */
  readonly topIndex: () => number;
}
