/** Landing a link or a key press on one block of a virtualized transcript.
 *
 * The block is usually not mounted, so the list scrolls to its index first,
 * then the drawn element is found and placed precisely, since its real height
 * can differ from the guess the first scroll used.
 */
import { type RefObject, useEffect } from "react";
import type { VirtualListHandle } from "./virtual-range";

/** Frames to wait for the target to be drawn after the first scroll. */
const MAX_FRAMES = 60;

/** Frames to hold the landed card in place while the blocks around it are
 *  measured for the first time and change height. */
const SETTLE_FRAMES = 30;

/** Input that means the reader took over the scroll. */
const TAKEOVER = ["wheel", "touchstart", "keydown", "pointerdown"] as const;

/**
 * Keep `card` where `place` put it for a few frames. Blocks drawn above it
 * but below the pane top (under a sticky bar) are measured after the jump,
 * and the list does not offset those, so the card would drift. Stops early
 * when the reader scrolls or types. Returns a cancel function.
 */
const settle = (card: HTMLElement, place: () => void) => {
  let want = card.getBoundingClientRect().top;
  let frames = 0;
  let raf = 0;
  const stop = () => {
    cancelAnimationFrame(raf);
    for (const t of TAKEOVER) {
      window.removeEventListener(t, stop, true);
    }
  };
  const tick = () => {
    frames += 1;
    if (!card.isConnected || frames > SETTLE_FRAMES) {
      stop();
      return;
    }
    if (Math.abs(card.getBoundingClientRect().top - want) > 1) {
      place();
      want = card.getBoundingClientRect().top;
    }
    raf = requestAnimationFrame(tick);
  };
  for (const t of TAKEOVER) {
    window.addEventListener(t, stop, { capture: true, passive: true });
  }
  raf = requestAnimationFrame(tick);
  return stop;
};

/** The first match of `selectors`, in priority order, or `el` itself made
 *  focusable when none matches. */
const focusTarget = (el: HTMLElement, selectors: readonly string[]) => {
  for (const selector of selectors) {
    const found = el.querySelector<HTMLElement>(selector);
    if (found) {
      return found;
    }
  }
  el.tabIndex = -1;
  return el;
};

/**
 * Scroll item `index` to `align`, wait until it is drawn, then place it and
 * move focus into it, so keyboard and screen-reader users are not left at
 * `<body>`. `anchor` picks the element inside the item to place (the item
 * itself when absent or unmatched); focus goes to the first `focusIn` match
 * inside it. With `hold`, the placed element is kept where it landed while
 * the blocks around it are measured. Returns a cancel function.
 */
export const landAt = ({
  align,
  anchor,
  focusIn,
  hold = false,
  index,
  list,
  onLand,
}: {
  readonly align: "start" | "center";
  readonly anchor?: string;
  readonly focusIn: readonly string[];
  readonly hold?: boolean;
  readonly index: number;
  readonly list: RefObject<VirtualListHandle | null>;
  readonly onLand?: () => void;
}) => {
  list.current?.scrollToIndex(index, align);
  let frames = 0;
  let raf = 0;
  let unsettle = () => {
    // nothing to hold yet
  };
  const land = () => {
    const item = list.current?.elementAt(index);
    if (!item) {
      frames += 1;
      if (frames < MAX_FRAMES) {
        raf = requestAnimationFrame(land);
      }
      return;
    }
    const target =
      (anchor ? item.querySelector<HTMLElement>(anchor) : null) ?? item;
    const place = () => target.scrollIntoView({ block: align });
    place();
    focusTarget(target, focusIn).focus({ preventScroll: true });
    if (hold) {
      unsettle = settle(target, place);
    }
    onLand?.();
  };
  raf = requestAnimationFrame(land);
  return () => {
    cancelAnimationFrame(raf);
    unsettle();
  };
};

/**
 * Scroll item `index` to the centre, then hand its drawn element to `onDrawn`.
 * Focus is left alone, so a search box keeps it while stepping. Returns a
 * cancel function.
 */
export const revealAt = ({
  index,
  list,
  onDrawn,
}: {
  readonly index: number;
  readonly list: RefObject<VirtualListHandle | null>;
  readonly onDrawn: (item: HTMLElement) => void;
}) => {
  list.current?.scrollToIndex(index, "center");
  let frames = 0;
  let raf = 0;
  const tick = () => {
    const item = list.current?.elementAt(index);
    if (item) {
      onDrawn(item);
      return;
    }
    frames += 1;
    if (frames < MAX_FRAMES) {
      raf = requestAnimationFrame(tick);
    }
  };
  raf = requestAnimationFrame(tick);
  return () => cancelAnimationFrame(raf);
};

/**
 * Bring item `index` into view and focus it, once per `target`. `target` is
 * the id the link names; `index` is where it sits in the list now, or -1 when
 * a filter hides it, in which case the jump waits until it is shown. Reading
 * on from there is left alone. Inside the drawn item, a tool card is what gets
 * placed and focused. `align` is where it lands: centred (the default) or at
 * the top, below anything sticky (the scroll pane's `scroll-padding-top`).
 * `done` belongs to the caller, so a view that remounts (a layout switch) does
 * not land on the same target again.
 */
export const useVirtualJump = ({
  align = "center",
  done,
  index,
  list,
  target,
}: {
  readonly align?: "start" | "center";
  readonly done: RefObject<string | null>;
  readonly index: number;
  readonly list: RefObject<VirtualListHandle | null>;
  readonly target: string | null;
}) => {
  useEffect(() => {
    // Not named, already landed, or hidden by a filter (`index` is -1).
    if (
      target === null ||
      done.current === target ||
      index < 0 ||
      !list.current
    ) {
      return;
    }
    done.current = target;
    let landed = false;
    const cancel = landAt({
      align,
      anchor: "[data-tool-use-id]",
      focusIn: ["button"],
      hold: true,
      index,
      list,
      onLand: () => {
        landed = true;
      },
    });
    return () => {
      cancel();
      // A jump cut short (the list changed or the view swapped) runs again.
      done.current = landed ? target : null;
    };
  }, [target, index, list, done, align]);
};
