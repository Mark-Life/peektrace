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
    const observer = new ResizeObserver(update);
    observer.observe(root);
    if (root.firstElementChild) {
      observer.observe(root.firstElementChild);
    }
    return () => observer.disconnect();
  }, [root, list]);
  return margin;
};

/** The item index that holds focus, or null when focus is outside the list. */
const useFocusedIndex = (list: HTMLElement | null) => {
  const [focused, setFocused] = useState<number | null>(null);
  useEffect(() => {
    if (!list) {
      return;
    }
    const onIn = (ev: FocusEvent) => {
      const item =
        ev.target instanceof Element
          ? ev.target.closest<HTMLElement>("[data-index]")
          : null;
      const index = Number(item?.dataset.index);
      setFocused(item && Number.isInteger(index) ? index : null);
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
  readonly testId?: string;
}) => {
  const root = useScrollRoot();
  const [list, setList] = useState<HTMLUListElement | null>(null);
  const scrollMargin = useScrollMargin(root, list);
  const focused = useFocusedIndex(list);
  const count = items.length;

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
    estimateSize: (i) => {
      const it = items[i];
      return it === undefined ? 0 : estimate(it);
    },
    getItemKey,
    getScrollElement: () => root,
    initialRect: initialRect(),
    overscan: count <= FULL_RENDER_MAX ? count : OVERSCAN,
    rangeExtractor,
    scrollMargin,
    useFlushSync: false,
  });

  useImperativeHandle(
    ref,
    () => ({
      elementAt: (index) =>
        list?.querySelector<HTMLElement>(`:scope > [data-index="${index}"]`) ??
        null,
      scrollToIndex: (index, align) => v.scrollToIndex(index, { align }),
    }),
    [v, list]
  );

  const lastMeasureKey = useRef(measureKey);
  useLayoutEffect(() => {
    if (lastMeasureKey.current === measureKey || !list) {
      return;
    }
    lastMeasureKey.current = measureKey;
    // Only re-anchor when the reader is inside the list; above it, the page
    // is already where they left it.
    const offset = v.scrollOffset ?? 0;
    const anchor =
      offset > scrollMargin ? v.getVirtualItemForOffset(offset) : undefined;
    v.measure();
    for (const el of list.querySelectorAll<HTMLLIElement>(
      ":scope > [data-index]"
    )) {
      v.measureElement(el);
    }
    if (anchor) {
      v.scrollToIndex(anchor.index, { align: "start" });
    }
  }, [measureKey, list, v, scrollMargin]);

  const drawn = v.getVirtualItems();
  const { flow, pinned } = splitPinned(drawn);
  const top = (flow[0]?.start ?? scrollMargin) - scrollMargin;

  // One keyed array for every drawn item: a block that moves between the run
  // and the pinned slot keeps its DOM node, and with it the focus.
  const item = (it: VirtualItem) => {
    const value = items[it.index];
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
        key={it.key}
        ref={v.measureElement}
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
      style={{ height: v.getTotalSize(), paddingTop: top }}
    >
      {drawn.map(item)}
    </ul>
  );
};
