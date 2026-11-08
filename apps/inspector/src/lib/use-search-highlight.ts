/** Paint search hits with the CSS Custom Highlight API.
 *
 * Ranges are built over the rendered text, so the row and markdown components
 * stay untouched. Browsers without `CSS.highlights` skip the paint; the counter
 * and stepping work the same. The current card gets `data-search-current` for
 * its outline: React never renders that attribute, so setting it by hand is
 * safe.
 */
import { type RefObject, useEffect, useRef } from "react";
import { textRanges } from "./history-search";

/** Highlight names, matched by the `::highlight()` rules in the search bar. */
export const HIT_HIGHLIGHT = "peektrace-search";
export const CURRENT_HIGHLIGHT = "peektrace-search-current";
export const CURRENT_ATTR = "data-search-current";

/** Enough to paint every hit on screen without stalling a huge transcript. */
const MAX_RANGES = 2000;

const registry = () =>
  typeof CSS !== "undefined" && "highlights" in CSS ? CSS.highlights : null;

/**
 * The rendered card for collapse id `id`. Chat cards carry the id as their DOM
 * id; an opened context chip carries it on its header, so take the enclosing
 * chip card. Table rows carry no id: take the row at the id's place in `ids`
 * (one `history-event` node per row, in render order).
 */
export const cardOf = ({
  container,
  id,
  ids,
}: {
  readonly container: HTMLElement;
  readonly id: string;
  readonly ids: readonly string[];
}) => {
  if (!id.startsWith("event:")) {
    const el = container.querySelector<HTMLElement>(`[id="${CSS.escape(id)}"]`);
    return (
      el?.closest<HTMLElement>('[data-testid="chat-chip-card"]') ?? el ?? null
    );
  }
  const at = ids.indexOf(id);
  return at < 0
    ? null
    : (container.querySelectorAll<HTMLElement>('[data-testid="history-event"]')[
        at
      ] ?? null);
};

/** The element holding the first rendered match of `needle` in `card`, or the
 *  card itself when no text matches (the hit is in a collapsed part). Scrolling
 *  this element also scrolls the card's own overflow boxes. */
export const firstHitIn = ({
  card,
  needle,
}: {
  readonly card: HTMLElement;
  readonly needle: string;
}) => {
  const walker = document.createTreeWalker(card, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (textRanges(node.nodeValue ?? "", needle).length > 0) {
      return node.parentElement ?? card;
    }
  }
  return card;
};

const paint = ({
  container,
  current,
  needle,
}: {
  readonly container: HTMLElement;
  readonly current: HTMLElement | null;
  readonly needle: string;
}) => {
  const hits = new Highlight();
  const inCurrent = new Highlight();
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  let count = 0;
  for (
    let node = walker.nextNode();
    node !== null && count < MAX_RANGES;
    node = walker.nextNode()
  ) {
    for (const [start, end] of textRanges(node.nodeValue ?? "", needle)) {
      const range = new Range();
      range.setStart(node, start);
      range.setEnd(node, end);
      (current?.contains(node) ? inCurrent : hits).add(range);
      count += 1;
    }
  }
  return { hits, inCurrent };
};

/** Highlight `query` inside `containerRef`, repainting as the DOM changes
 *  (a card opening, a live append). */
export const useSearchHighlight = ({
  containerRef,
  currentId,
  ids,
  query,
}: {
  readonly containerRef: RefObject<HTMLElement | null>;
  readonly currentId: string | null;
  /** Transcript collapse ids in render order, for table-row lookup. */
  readonly ids: readonly string[];
  readonly query: string;
}) => {
  // Read through a ref: a live append changes `ids` but needs no new observer.
  const idsRef = useRef(ids);
  useEffect(() => {
    idsRef.current = ids;
  }, [ids]);

  useEffect(() => {
    const container = containerRef.current;
    const needle = query.toLowerCase();
    if (container === null || needle.length === 0) {
      return;
    }
    const reg = registry();
    let marked: HTMLElement | null = null;
    let frame = 0;

    const run = () => {
      frame = 0;
      const current =
        currentId === null
          ? null
          : cardOf({ container, id: currentId, ids: idsRef.current });
      if (marked !== current) {
        marked?.removeAttribute(CURRENT_ATTR);
        current?.setAttribute(CURRENT_ATTR, "");
        marked = current;
      }
      if (reg) {
        const { hits, inCurrent } = paint({ container, current, needle });
        reg.set(HIT_HIGHLIGHT, hits);
        reg.set(CURRENT_HIGHLIGHT, inCurrent);
      }
    };
    const schedule = () => {
      if (frame === 0) {
        frame = requestAnimationFrame(run);
      }
    };

    run();
    const observer = new MutationObserver(schedule);
    observer.observe(container, {
      characterData: true,
      childList: true,
      subtree: true,
    });
    return () => {
      observer.disconnect();
      if (frame !== 0) {
        cancelAnimationFrame(frame);
      }
      marked?.removeAttribute(CURRENT_ATTR);
      reg?.delete(HIT_HIGHLIGHT);
      reg?.delete(CURRENT_HIGHLIGHT);
    };
  }, [containerRef, currentId, query]);
};
