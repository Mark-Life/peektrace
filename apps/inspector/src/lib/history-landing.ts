/** Hooks the history section uses to land jumps: the sticky toolbar's height,
 *  which jumps and keyboard focus land below, and the subagents section, which
 *  sits outside the virtual list.
 */
import { type RefObject, useEffect, useLayoutEffect, useState } from "react";
import type { HistoryJump } from "./history-jump";

/** The toolbar sticks only on screens at least this tall; on a short one (a
 *  phone on its side, 400% zoom) it would cover too much of the view. Keep in
 *  step with the toolbar's `[@media(min-height:500px)]:sticky`. */
const STICKY_MEDIA = "(min-height: 500px)";

/** Height of `bar` while it sticks to the pane top; 0 when it does not
 *  stick, so a jump has nothing to land below. */
export const useStickyHeight = (bar: HTMLElement | null) => {
  const [height, setHeight] = useState(0);
  useLayoutEffect(() => {
    if (!bar) {
      return;
    }
    const media = window.matchMedia(STICKY_MEDIA);
    let box = bar.getBoundingClientRect().height;
    const update = () => {
      const next = media.matches ? Math.round(box) : 0;
      setHeight((prev) => (prev === next ? prev : next));
    };
    const resize = new ResizeObserver(([entry]) => {
      box = entry?.borderBoxSize[0]?.blockSize ?? box;
      update();
    });
    resize.observe(bar);
    media.addEventListener("change", update);
    update();
    return () => {
      resize.disconnect();
      media.removeEventListener("change", update);
    };
  }, [bar]);
  return height;
};

/** Keep `px` clear at the top of the scroll pane while the history is shown,
 *  so anything the browser scrolls to (Tab focus, find in page, a landed
 *  jump) lands below the sticky toolbar, not under it. */
export const useScrollPadding = (root: HTMLElement | null, px: number) => {
  useEffect(() => {
    if (!root) {
      return;
    }
    root.style.scrollPaddingTop = `${px}px`;
    return () => {
      root.style.scrollPaddingTop = "";
    };
  }, [root, px]);
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
  const key = jump?.target.kind === "subagents" ? jump.key : null;
  useEffect(() => {
    if (key === null || done.current === key) {
      return;
    }
    const box = section.current?.querySelector<HTMLElement>(
      '[data-testid="subagents"]'
    );
    if (!box) {
      return;
    }
    done.current = key;
    box.scrollIntoView({ block: "start" });
    const heading = box.querySelector<HTMLElement>("h3") ?? box;
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
  }, [key, done, section]);
};
