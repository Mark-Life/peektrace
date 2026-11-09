/** Paint search hits with the CSS Custom Highlight API.
 *
 * Ranges are built over the rendered text, so the row and markdown components
 * stay untouched. Browsers without `CSS.highlights` skip the paint; the counter
 * and stepping work the same. The current card gets `data-search-current` for
 * its outline: React never renders that attribute, so setting it by hand is
 * safe.
 */
import { type RefObject, useEffect } from "react";
import { rangesIn, textRanges } from "./history-search";

/** Highlight names, matched by the `::highlight()` rules in the search bar. */
export const HIT_HIGHLIGHT = "peektrace-search";
export const CURRENT_HIGHLIGHT = "peektrace-search-current";
export const CURRENT_ATTR = "data-search-current";

/** Enough to paint every hit on screen without stalling a huge transcript. */
const MAX_RANGES = 2000;

const registry = () =>
  typeof CSS !== "undefined" && "highlights" in CSS ? CSS.highlights : null;

/**
 * The rendered card for collapse id `id`, or null when it is not drawn. Chat
 * cards carry the id as their DOM id; an opened context chip carries it on its
 * header, so take the enclosing chip card. Table rows carry it as
 * `data-collapse-id`.
 */
export const cardOf = ({
  container,
  id,
}: {
  readonly container: HTMLElement;
  readonly id: string;
}) => {
  const sel = CSS.escape(id);
  const el = container.querySelector<HTMLElement>(
    `[id="${sel}"], [data-collapse-id="${sel}"]`
  );
  return (
    el?.closest<HTMLElement>('[data-testid="chat-chip-card"]') ?? el ?? null
  );
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

/** Text nodes under `root` in DOM order, skipping the subtree of `skip`. */
function* textNodes(root: HTMLElement, skip: HTMLElement | null = null) {
  const walker = document.createTreeWalker(
    root,
    // biome-ignore lint/suspicious/noBitwiseOperators: NodeFilter flags
    NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT,
    {
      acceptNode: (node) => {
        if (node === skip) {
          return NodeFilter.FILTER_REJECT;
        }
        return node.nodeType === Node.TEXT_NODE
          ? NodeFilter.FILTER_ACCEPT
          : NodeFilter.FILTER_SKIP;
      },
    }
  );
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    yield node;
  }
}

const toHighlight = (
  ranges: readonly {
    readonly end: number;
    readonly node: Node;
    readonly start: number;
  }[]
) => {
  const out = new Highlight();
  for (const { end, node, start } of ranges) {
    const range = new Range();
    range.setStart(node, start);
    range.setEnd(node, end);
    out.add(range);
  }
  return out;
};

/** The current card is painted in full first, so the cap on the rest never
 *  drops its highlight. */
const paint = ({
  container,
  current,
  needle,
}: {
  readonly container: HTMLElement;
  readonly current: HTMLElement | null;
  readonly needle: string;
}) => ({
  hits: toHighlight(
    rangesIn({ max: MAX_RANGES, needle, nodes: textNodes(container, current) })
  ),
  inCurrent: toHighlight(
    current === null ? [] : rangesIn({ needle, nodes: textNodes(current) })
  ),
});

/** Highlight `query` inside `containerRef`, repainting as the DOM changes
 *  (a card opening, a live append). */
export const useSearchHighlight = ({
  containerRef,
  currentId,
  query,
}: {
  readonly containerRef: RefObject<HTMLElement | null>;
  readonly currentId: string | null;
  readonly query: string;
}) => {
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
        currentId === null ? null : cardOf({ container, id: currentId });
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
