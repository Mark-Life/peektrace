/** Keyboard list-selection + viewport windowing for terminal lists.
 *
 * OpenTUI's `<select>` can't color individual cells, so the forensic lists here
 * are hand-rolled: this hook owns the selected index (arrow / j-k / Home / End)
 * and `windowSlice` keeps the selection inside a fixed-height viewport.
 */
import { useKeyboard } from "@opentui/react";
import { useEffect, useState } from "react";

/**
 * Track a selected index over a list of `count` items. Navigation keys are only
 * honored while `active` (the owning pane has focus). Returns the index and a
 * setter (for click/jump-to selection).
 */
export const useListSelection = (
  count: number,
  active: boolean
): readonly [number, (index: number) => void] => {
  const [index, setIndex] = useState(0);

  useEffect(() => {
    if (index > count - 1) {
      setIndex(Math.max(0, count - 1));
    }
  }, [count, index]);

  useKeyboard((key) => {
    if (!active || count === 0) {
      return;
    }
    switch (key.name) {
      case "up":
      case "k":
        setIndex((i) => Math.max(0, i - 1));
        break;
      case "down":
      case "j":
        setIndex((i) => Math.min(count - 1, i + 1));
        break;
      case "home":
        setIndex(0);
        break;
      case "end":
        setIndex(Math.max(0, count - 1));
        break;
      default:
        break;
    }
  });

  return [Math.min(index, Math.max(0, count - 1)), setIndex] as const;
};

/** Inclusive-exclusive viewport bounds keeping `index` visible in `visible` rows. */
export interface Window {
  readonly end: number;
  readonly start: number;
}

/**
 * Compute the slice `[start, end)` of a `count`-length list that keeps `index`
 * visible within a `visible`-row viewport, scrolling only when the selection
 * would fall outside the current window.
 */
export const windowSlice = (
  index: number,
  count: number,
  visible: number
): Window => {
  if (count <= visible) {
    return { start: 0, end: count };
  }
  const half = Math.floor(visible / 2);
  let start = index - half;
  start = Math.max(0, Math.min(start, count - visible));
  return { start, end: start + visible };
};
