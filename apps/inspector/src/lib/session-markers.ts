/** Stats markers for the open transcript, keyed by tool-use id.
 *
 * The reader and the tables agree glyph for glyph because both read the same
 * `detector` ids from core. Markers are an annotation, never the content: a
 * failed or pending marker query leaves the transcript exactly as it was.
 */
import { Result, useAtomValue } from "@effect-atom/atom-react";
import type { SessionMarker } from "@workspace/core/services/stats/schema";
import { useEffect, useMemo, useRef } from "react";
import { useSessionMarkers } from "./stats-atoms";

/** Marks for one tool call, in the order core emitted them. */
export type ToolMarks = ReadonlyMap<string, readonly SessionMarker[]>;

const EMPTY: ToolMarks = new Map();

const indexOf = (markers: readonly SessionMarker[]): ToolMarks => {
  const map = new Map<string, SessionMarker[]>();
  for (const marker of markers) {
    if (marker.toolUseId === null) {
      continue;
    }
    const bucket = map.get(marker.toolUseId);
    if (bucket) {
      bucket.push(marker);
    } else {
      map.set(marker.toolUseId, [marker]);
    }
  }
  return map;
};

/**
 * Move focus to a card's toggle, or the card itself when it has none: the
 * route swap dropped the focused stats link, which would leave keyboard and
 * screen-reader users at `<body>`. The scroll already placed the card.
 */
const focusCard = (card: HTMLElement): void => {
  const trigger = card.querySelector<HTMLElement>("button");
  if (trigger) {
    trigger.focus({ preventScroll: true });
    return;
  }
  card.tabIndex = -1;
  card.focus({ preventScroll: true });
};

/**
 * Bring the card of call `id` (a tool-use id) into view once it is drawn, so
 * a stats link lands on the call it names, an unclassified error with no mark
 * of its own included. `drawn` is the list the transcript renders: a filter
 * or search that hid the card and is then cleared draws it, and the scroll
 * runs then. Focus follows the scroll. Runs once per call: reading on from
 * there is left alone.
 */
export const useScrollToCall = (
  id: string | null,
  drawn: readonly unknown[]
): void => {
  const done = useRef<string | null>(null);
  useEffect(() => {
    if (id === null || done.current === id || drawn.length === 0) {
      return;
    }
    const card = document.querySelector(
      `[data-tool-use-id="${CSS.escape(id)}"]`
    );
    if (card instanceof HTMLElement) {
      done.current = id;
      card.scrollIntoView({ block: "center" });
      focusCard(card);
    }
  }, [id, drawn]);
};

/** Markers for one session id, indexed by the transcript's own tool-use id. */
export const useToolMarks = (sid: string): ToolMarks => {
  const result = useAtomValue(useSessionMarkers(sid));
  return useMemo(
    () => (Result.isSuccess(result) ? indexOf(result.value.markers) : EMPTY),
    [result]
  );
};
