/** History search state: the per-session query and the current hit.
 *
 * Like browser find, the query belongs to one session view and resets when
 * another session opens. The route keys the session view by session, so a
 * switch remounts it with an empty query; the `sid` check is only a guard for
 * a caller that keeps the view mounted.
 * Filtering reads the deferred query, so typing stays responsive on long
 * transcripts. Stepping never writes the persisted open-row state: the card
 * it opens stays open only while it is the current hit.
 */
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useRef,
  useState,
} from "react";
import { type StepDir, stepHit } from "./history-search";
import { cardOf, firstHitIn, useSearchHighlight } from "./use-search-highlight";

interface QueryState {
  readonly query: string;
  readonly sid: string;
}

/** The query state for session `sid`: kept for the same session, cleared for
 *  another one. */
export const queryStateFor = (state: QueryState, sid: string): QueryState =>
  state.sid === sid ? state : { query: "", sid };

/** The search box value, cleared whenever `sid` changes. */
export const useSearchQuery = (sid: string) => {
  const [state, setState] = useState<QueryState>({ query: "", sid });
  const next = queryStateFor(state, sid);
  if (next !== state) {
    setState(next);
  }
  const setQuery = useCallback(
    (query: string) => setState((s) => ({ ...s, query })),
    []
  );
  const deferredQuery = useDeferredValue(next.query);
  return { deferredQuery, query: next.query, setQuery };
};

/**
 * Step through `hits` (collapse ids in render order): open the card, scroll its
 * first match to the middle and paint it as current. The current hit is
 * tracked by id so a live append keeps it; it falls back to none when the id
 * leaves `hits` (a layout or kind change) or `resetKey` changes (a new query).
 * Render with the returned `isOpen` / `onToggle`, which add the card the
 * stepper opened on top of the persisted state.
 */
export const useSearchStepper = ({
  hits,
  isOpen,
  onToggle,
  query,
  resetKey,
}: {
  readonly hits: readonly string[];
  readonly isOpen: (id: string) => boolean;
  readonly onToggle: (id: string, open: boolean) => void;
  /** The query the hits were filtered by; highlights follow it. */
  readonly query: string;
  readonly resetKey: string;
}) => {
  const [currentId, setCurrentId] = useState<string | null>(null);
  /** The current hit, when it was closed before the step landed on it. */
  const [openedId, setOpenedId] = useState<string | null>(null);
  const [key, setKey] = useState(resetKey);
  if (key !== resetKey) {
    setKey(resetKey);
    setCurrentId(null);
    setOpenedId(null);
  }
  const current = currentId === null ? -1 : hits.indexOf(currentId);
  const liveId = current < 0 ? null : currentId;
  const liveOpened = liveId !== null && openedId === liveId ? openedId : null;

  const containerRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef(0);
  useEffect(() => () => cancelAnimationFrame(frameRef.current), []);

  const onStep = (dir: StepDir) => {
    const next = stepHit({ current: liveId, dir, hits });
    if (next === null) {
      return;
    }
    setOpenedId(isOpen(next) ? null : next);
    setCurrentId(next);
    // The step's render commits before the next frame, so the card is open.
    cancelAnimationFrame(frameRef.current);
    frameRef.current = requestAnimationFrame(() => {
      const container = containerRef.current;
      const card =
        container === null ? null : cardOf({ container, id: next, ids: hits });
      if (card !== null) {
        // Jump like browser find: a smooth scroll is slow over long
        // transcripts and can stop short while skipped cards render.
        firstHitIn({ card, needle: query.toLowerCase() }).scrollIntoView({
          behavior: "instant",
          block: "center",
        });
      }
    });
  };

  useSearchHighlight({ containerRef, currentId: liveId, ids: hits, query });

  // Rows are memoized on the toggle handler: read the opened id through a ref
  // so a step does not hand every row a new handler.
  const openedRef = useRef(liveOpened);
  useEffect(() => {
    openedRef.current = liveOpened;
  }, [liveOpened]);
  const onToggleOpen = useCallback(
    (id: string, open: boolean) => {
      if (id === openedRef.current) {
        setOpenedId(null);
      }
      onToggle(id, open);
    },
    [onToggle]
  );
  const isOpenNow = (id: string) => id === liveOpened || isOpen(id);

  return {
    containerRef,
    count: hits.length,
    current,
    isOpen: isOpenNow,
    onStep,
    onToggle: onToggleOpen,
  };
};
