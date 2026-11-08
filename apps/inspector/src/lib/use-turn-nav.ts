/** `[` and `]` in an open transcript: scroll to the previous or next turn and
 * move focus there, so a screen reader announces it and the item stays
 * mounted while the reader moves on. */
import { type RefObject, useEffect, useRef } from "react";
import { useShortcut } from "./shortcuts";
import { stepStop } from "./turn-nav";
import type { VirtualListHandle } from "./virtual-range";

/** Frames to wait for the target to be drawn after the first scroll. */
const MAX_FRAMES = 60;

/** The element inside a drawn item that takes focus. */
const focusTarget = (item: HTMLElement) => {
  const own =
    item.querySelector<HTMLElement>("[data-turn-stop]") ??
    item.querySelector<HTMLElement>("button");
  if (own) {
    return own;
  }
  item.tabIndex = -1;
  return item;
};

/** Bind `[` / `]` to the `stops` (item indexes) of the mounted list. */
export const useTurnNav = ({
  list,
  stops,
}: {
  readonly list: RefObject<VirtualListHandle | null>;
  readonly stops: readonly number[];
}) => {
  const raf = useRef(0);
  useEffect(() => () => cancelAnimationFrame(raf.current), []);

  const go = (dir: "next" | "prev") => {
    const handle = list.current;
    if (!handle) {
      return;
    }
    const index = stepStop({ anchor: handle.topIndex(), dir, stops });
    if (index === null) {
      return;
    }
    handle.scrollToIndex(index, "start");
    cancelAnimationFrame(raf.current);
    let frames = 0;
    const land = () => {
      const item = list.current?.elementAt(index);
      if (item) {
        item.scrollIntoView({ block: "start" });
        focusTarget(item).focus({ preventScroll: true });
        return;
      }
      frames += 1;
      if (frames < MAX_FRAMES) {
        raf.current = requestAnimationFrame(land);
      }
    };
    raf.current = requestAnimationFrame(land);
  };

  useShortcut("prevTurn", () => go("prev"));
  useShortcut("nextTurn", () => go("next"));
};
