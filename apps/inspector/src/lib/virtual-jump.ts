/** Landing a link on one block of a virtualized transcript.
 *
 * The block a link names is usually not mounted, so the list scrolls to its
 * index first, then the drawn element is found and placed precisely, since
 * its real height can differ from the guess the first scroll used.
 */
import { type RefObject, useEffect } from "react";
import type { VirtualListHandle } from "./virtual-range";

/** Frames to wait for the target to be drawn after the first scroll. */
const MAX_FRAMES = 60;

/**
 * Move focus to a card's toggle, or the card itself when it has none: the
 * route swap dropped the focused link, which would leave keyboard and
 * screen-reader users at `<body>`. The scroll already placed the card.
 */
const focusCard = (card: HTMLElement) => {
  const trigger = card.querySelector<HTMLElement>("button");
  if (trigger) {
    trigger.focus({ preventScroll: true });
    return;
  }
  card.tabIndex = -1;
  card.focus({ preventScroll: true });
};

/** Whether a jump to `target` is due: one is named, it has not landed yet,
 *  and a filter does not hide it (`index` is -1 then). */
export const jumpDue = ({
  done,
  index,
  target,
}: {
  readonly done: string | null;
  readonly index: number;
  readonly target: string | null;
}) => target !== null && done !== target && index >= 0;

/** What `done` holds once a jump ends: the target when it landed, else null,
 *  so a jump cut short (the list changed or the view swapped) runs again. */
export const doneAfter = ({
  landed,
  target,
}: {
  readonly landed: boolean;
  readonly target: string;
}) => (landed ? target : null);

/**
 * Bring item `index` into view and focus it, once per `target`. `target` is
 * the id the link names; `index` is where it sits in the list now, or -1 when
 * a filter hides it, in which case the jump waits until it is shown. Reading
 * on from there is left alone. Inside the drawn item, a tool card is what gets
 * centred and focused. `done` belongs to the caller, so a view that remounts
 * (a layout switch) does not land on the same target again.
 */
export const useVirtualJump = ({
  done,
  index,
  list,
  target,
}: {
  readonly done: RefObject<string | null>;
  readonly index: number;
  readonly list: RefObject<VirtualListHandle | null>;
  readonly target: string | null;
}) => {
  useEffect(() => {
    if (
      target === null ||
      !jumpDue({ done: done.current, index, target }) ||
      !list.current
    ) {
      return;
    }
    done.current = target;
    list.current.scrollToIndex(index, "center");
    let frames = 0;
    let raf = 0;
    let landed = false;
    const land = () => {
      const item = list.current?.elementAt(index);
      if (item) {
        landed = true;
        const card =
          item.querySelector<HTMLElement>("[data-tool-use-id]") ?? item;
        card.scrollIntoView({ block: "center" });
        focusCard(card);
        return;
      }
      frames += 1;
      if (frames < MAX_FRAMES) {
        raf = requestAnimationFrame(land);
      }
    };
    raf = requestAnimationFrame(land);
    return () => {
      cancelAnimationFrame(raf);
      done.current = doneAfter({ landed, target });
    };
  }, [target, index, list, done]);
};
