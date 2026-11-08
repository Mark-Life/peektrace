/** Hooks the history section uses to land summary jumps: the sticky
 *  toolbar's height, which jumps land below, and the subagents section, which
 *  sits outside the virtual list.
 */
import { type RefObject, useEffect, useLayoutEffect, useState } from "react";
import type { HistoryJump } from "./history-jump";

/** Height of `bar` while it sticks to the pane top; 0 when it does not
 *  stick, so a jump has nothing to land below. */
export const useStickyHeight = (bar: HTMLElement | null) => {
  const [height, setHeight] = useState(0);
  useLayoutEffect(() => {
    if (!bar) {
      return;
    }
    const update = () => {
      const next =
        getComputedStyle(bar).position === "sticky"
          ? Math.round(bar.getBoundingClientRect().height)
          : 0;
      setHeight((prev) => (prev === next ? prev : next));
    };
    update();
    const resize = new ResizeObserver(update);
    resize.observe(bar);
    return () => resize.disconnect();
  }, [bar]);
  return height;
};

/** Bring the subagents section up when a jump names it, once per jump. */
export const useSubagentsJump = ({
  done,
  jump,
  section,
}: {
  readonly done: RefObject<string | null>;
  readonly jump: HistoryJump | null;
  readonly section: RefObject<HTMLElement | null>;
}) => {
  useEffect(() => {
    if (jump?.target.kind !== "subagents" || done.current === jump.key) {
      return;
    }
    const box = section.current?.querySelector<HTMLElement>(
      '[data-testid="subagents"]'
    );
    if (!box) {
      return;
    }
    done.current = jump.key;
    box.scrollIntoView({ block: "start" });
    const heading = box.querySelector<HTMLElement>("h3") ?? box;
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
  }, [jump, done, section]);
};
