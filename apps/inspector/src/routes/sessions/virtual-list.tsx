/** A long list that only mounts what is on screen.
 *
 * Scrolls with the shell's content pane, so the transcript keeps its place in
 * the page instead of becoming a box of its own. Items sit in normal flow
 * below a spacer, which keeps each layout's own width and centring; heights
 * come from `estimate` until an item is drawn and measured.
 */
import {
  defaultRangeExtractor,
  type Range,
  useVirtualizer,
  type VirtualItem,
} from "@tanstack/react-virtual";
import { cn } from "@workspace/ui/lib/utils";
import {
  type CSSProperties,
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useScrollRoot } from "../../lib/scroll-root";
import {
  keepIndex,
  splitPinned,
  type VirtualListHandle,
} from "../../lib/virtual-range";

/** Items drawn past each edge of the screen: enough that a fast scroll or a
 *  Tab press rarely reaches an unmounted block. */
const OVERSCAN = 10;

/** Lists up to this long mount every item, so browser find-in-page and
 *  screen-reader browse mode still reach the whole transcript. */
const FULL_RENDER_MAX = 150;

/** A viewport to plan the first render against, before the pane is measured. */
const FALLBACK_HEIGHT = 900;

const initialRect = () => ({
  height: typeof window === "undefined" ? FALLBACK_HEIGHT : window.innerHeight,
  width: 0,
});

/** Calls `update` whenever something can move the list down: an earlier
 *  sibling of the list or of any ancestor up to the pane resizes, appears or
 *  goes, or the pane itself resizes. The list's own growth is not watched. */
const watchAbove = (
  root: HTMLElement,
  list: HTMLElement,
  update: () => void
) => {
  const resize = new ResizeObserver(update);
  const wire = () => {
    resize.disconnect();
    resize.observe(root);
    for (let n: Element | null = list; n && n !== root; n = n.parentElement) {
      for (let s = n.previousElementSibling; s; s = s.previousElementSibling) {
        resize.observe(s);
      }
    }
  };
  const mutate = new MutationObserver(() => {
    wire();
    update();
  });
  for (
    let p: HTMLElement | null = list.parentElement;
    p;
    p = p === root ? null : p.parentElement
  ) {
    mutate.observe(p, { childList: true });
  }
  wire();
  return () => {
    resize.disconnect();
    mutate.disconnect();
  };
};

/** Where the list starts inside the scroll pane's content, kept current as
 *  anything above it (verdict, timeline, banners) changes height. */
const useScrollMargin = (
  root: HTMLElement | null,
  list: HTMLElement | null
) => {
  const [margin, setMargin] = useState(0);
  useLayoutEffect(() => {
    if (!(root && list)) {
      return;
    }
    const update = () => {
      const next = Math.round(
        list.getBoundingClientRect().top -
          root.getBoundingClientRect().top +
          root.scrollTop
      );
      setMargin((prev) => (prev === next ? prev : next));
    };
    update();
    return watchAbove(root, list, update);
  }, [root, list]);
  return margin;
};

/** The key of the item that holds focus, or null when focus is outside the
 *  list. A key, not an index: rows inserted above move the focused item. */
const useFocusedKey = (list: HTMLElement | null) => {
  const [focused, setFocused] = useState<string | null>(null);
  useEffect(() => {
    if (!list) {
      return;
    }
    const onIn = (ev: FocusEvent) => {
      const item =
        ev.target instanceof Element
          ? ev.target.closest<HTMLElement>("[data-key]")
          : null;
      setFocused(item?.dataset.key ?? null);
    };
    const onOut = (ev: FocusEvent) => {
      if (
        !(ev.relatedTarget instanceof Node && list.contains(ev.relatedTarget))
      ) {
        setFocused(null);
      }
    };
    list.addEventListener("focusin", onIn);
    list.addEventListener("focusout", onOut);
    return () => {
      list.removeEventListener("focusin", onIn);
      list.removeEventListener("focusout", onOut);
    };
  }, [list]);
  return focused;
};

/** A virtualized list. `keys` holds one unique, stable key per item: measured
 *  heights are cached by key, so they survive filtering and appends. */
export const VirtualList = <T,>({
  className,
  estimate,
  itemClassName,
  items,
  keys,
  measureKey,
  ref,
  render,
  scrollPaddingStart = 0,
  testId,
}: {
  readonly className?: string;
  /** Guessed height of an item before it is measured. */
  readonly estimate: (item: T) => number;
  readonly itemClassName?: string;
  readonly items: readonly T[];
  readonly keys: readonly string[];
  /** Changes when many heights change at once (expand or collapse all): every
   *  height is guessed again and the reader stays on the same block. */
  readonly measureKey?: unknown;
  readonly ref?: Ref<VirtualListHandle>;
  readonly render: (item: T) => ReactNode;
  /** Height covered at the top of the pane (a sticky toolbar): an item
   *  scrolled to the start lands below it. */
  readonly scrollPaddingStart?: number;
  readonly testId?: string;
}) => {
  const root = useScrollRoot();
  const [list, setList] = useState<HTMLUListElement | null>(null);
  const count = items.length;
  const virtual = count > FULL_RENDER_MAX;
  const scrollMargin = useScrollMargin(root, virtual ? list : null);
  const focusedKey = useFocusedKey(list);
  const focused = focusedKey === null ? null : keys.indexOf(focusedKey);

  const getItemKey = useCallback((i: number) => keys[i] ?? i, [keys]);
  const rangeExtractor = useCallback(
    (range: Range) =>
      keepIndex({
        count: range.count,
        indexes: defaultRangeExtractor(range),
        keep: focused,
      }),
    [focused]
  );

  const v = useVirtualizer<HTMLElement, HTMLLIElement>({
    count,
    enabled: virtual,
    estimateSize: (i) => {
      const it = items[i];
      return it === undefined ? 0 : estimate(it);
    },
    getItemKey,
    getScrollElement: () => root,
    initialRect: initialRect(),
    overscan: OVERSCAN,
    rangeExtractor,
    scrollMargin,
    scrollPaddingStart,
    useFlushSync: false,
  });

  const elementAt = useCallback(
    (index: number) =>
      list?.querySelector<HTMLElement>(`:scope > [data-index="${index}"]`) ??
      null,
    [list]
  );
  useImperativeHandle(
    ref,
    () => ({
      elementAt,
      scrollToIndex: (index, align) => {
        if (virtual) {
          v.scrollToIndex(index, { align });
          return;
        }
        elementAt(index)?.scrollIntoView({ block: align });
      },
    }),
    [v, virtual, elementAt]
  );

  const lastMeasureKey = useRef(measureKey);
  useLayoutEffect(() => {
    if (lastMeasureKey.current === measureKey || !list) {
      return;
    }
    lastMeasureKey.current = measureKey;
    if (!virtual) {
      return;
    }
    // Only re-anchor when the reader is inside the list; above it, the page
    // is already where they left it.
    const offset = v.scrollOffset ?? 0;
    const anchor =
      offset > scrollMargin
        ? v.getVirtualItemForOffset(offset + scrollPaddingStart)
        : undefined;
    v.measure();
    for (const el of list.querySelectorAll<HTMLLIElement>(
      ":scope > [data-index]"
    )) {
      v.measureElement(el);
    }
    if (anchor) {
      v.scrollToIndex(anchor.index, { align: "start" });
    }
  }, [measureKey, list, v, virtual, scrollMargin, scrollPaddingStart]);

  // Small lists mount every item in plain flow: nothing to measure or place.
  const drawn: readonly Pick<VirtualItem, "index" | "start">[] = virtual
    ? v.getVirtualItems()
    : items.map((_, index) => ({ index, start: 0 }));
  const { flow, pinned } = splitPinned(drawn);
  const top = virtual ? (flow[0]?.start ?? scrollMargin) - scrollMargin : 0;

  // One keyed array for every drawn item: a block that moves between the run
  // and the pinned slot, or between the two modes, keeps its DOM node, and
  // with it the focus.
  const item = (it: Pick<VirtualItem, "index" | "start">) => {
    const value = items[it.index];
    const key = keys[it.index] ?? String(it.index);
    const style: CSSProperties | undefined = pinned.includes(it)
      ? {
          left: 0,
          position: "absolute",
          right: 0,
          top: it.start - scrollMargin,
        }
      : undefined;
    return value === undefined ? null : (
      <li
        aria-posinset={it.index + 1}
        aria-setsize={count}
        className={itemClassName}
        data-index={it.index}
        data-key={key}
        key={key}
        ref={virtual ? v.measureElement : undefined}
        style={style}
      >
        {render(value)}
      </li>
    );
  };

  return (
    <ul
      className={cn("relative", className)}
      data-testid={testId}
      ref={setList}
      style={
        virtual ? { height: v.getTotalSize(), paddingTop: top } : undefined
      }
    >
      {drawn.map(item)}
    </ul>
  );
};
