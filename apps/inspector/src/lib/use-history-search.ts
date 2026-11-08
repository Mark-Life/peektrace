/** History search state: the per-session query and the current hit.
 *
 * Like browser find, the query belongs to one session view and resets when
 * another session opens (the history view stays mounted across sessions).
 * Filtering reads the deferred query, so typing stays responsive on long
 * transcripts.
 */
import { useDeferredValue, useEffect, useRef, useState } from "react";
import { type StepDir, stepHit } from "./history-search";
import { cardOf, useSearchHighlight } from "./use-search-highlight";

/** The search box value, cleared whenever `sid` changes. */
export const useSearchQuery = (sid: string) => {
  const [query, setQuery] = useState("");
  const [querySid, setQuerySid] = useState(sid);
  if (querySid !== sid) {
    setQuerySid(sid);
    setQuery("");
  }
  return { deferredQuery: useDeferredValue(query), query, setQuery };
};

/**
 * Step through `hits` (collapse ids in render order): open the card, scroll it
 * to the middle and paint it as current. The current hit is tracked by id so a
 * live append keeps it; it falls back to none when the id leaves `hits` (a
 * layout or kind change) or `resetKey` changes (a new query or session).
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
  const [step, setStep] = useState(0);
  const [key, setKey] = useState(resetKey);
  if (key !== resetKey) {
    setKey(resetKey);
    setCurrentId(null);
  }
  const current = currentId === null ? -1 : hits.indexOf(currentId);
  const liveId = current < 0 ? null : currentId;

  const onStep = (dir: StepDir) => {
    const next = stepHit({ current: liveId, dir, hits });
    if (next === null) {
      return;
    }
    if (!isOpen(next)) {
      onToggle(next, true);
    }
    setCurrentId(next);
    setStep((n) => n + 1);
  };

  const containerRef = useRef<HTMLDivElement>(null);
  useSearchHighlight({ containerRef, currentId: liveId, ids: hits, query });

  const hitsRef = useRef(hits);
  useEffect(() => {
    hitsRef.current = hits;
  }, [hits]);
  // `step` re-runs this when Enter lands on the same lone hit again.
  // biome-ignore lint/correctness/useExhaustiveDependencies: step is the trigger
  useEffect(() => {
    const container = containerRef.current;
    if (container === null || liveId === null) {
      return;
    }
    cardOf({ container, id: liveId, ids: hitsRef.current })?.scrollIntoView({
      block: "center",
    });
  }, [liveId, step]);

  return { containerRef, count: hits.length, current, onStep };
};
