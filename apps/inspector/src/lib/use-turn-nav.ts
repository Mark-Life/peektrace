/** `[` and `]` in an open transcript: scroll to the previous or next turn and
 * move focus there, so a screen reader announces it and the item stays
 * mounted while the reader moves on. */
import { type RefObject, useEffect, useRef } from "react";
import { useShortcut } from "./shortcuts";
import { stepStop } from "./turn-nav";
import { landAt } from "./virtual-jump";
import type { VirtualListHandle } from "./virtual-range";

/** Bind `[` / `]` to the `stops` (item indexes) of the mounted list. */
export const useTurnNav = ({
  list,
  stops,
}: {
  readonly list: RefObject<VirtualListHandle | null>;
  readonly stops: readonly number[];
}) => {
  const cancel = useRef<() => void>(() => undefined);
  useEffect(() => () => cancel.current(), []);

  const go = (dir: "next" | "prev") => {
    const handle = list.current;
    if (!handle) {
      return;
    }
    const index = stepStop({ anchor: handle.topIndex(), dir, stops });
    if (index === null) {
      return;
    }
    cancel.current();
    cancel.current = landAt({
      align: "start",
      focusIn: ["[data-turn-stop]", "button"],
      index,
      list,
    });
  };

  useShortcut("prevTurn", () => go("prev"));
  useShortcut("nextTurn", () => go("next"));
};
