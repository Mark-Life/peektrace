/** Stats markers for the open transcript, keyed by tool-use id.
 *
 * The reader and the tables agree glyph for glyph because both read the same
 * `detector` ids from core. Markers are an annotation, never the content: a
 * failed or pending marker query leaves the transcript exactly as it was.
 */
import { Result, useAtomValue } from "@effect-atom/atom-react";
import type { SessionMarker } from "@workspace/core/services/stats/schema";
import { useMemo } from "react";
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

/** Markers for one session id, indexed by the transcript's own tool-use id. */
export const useToolMarks = (sid: string): ToolMarks => {
  const result = useAtomValue(useSessionMarkers(sid));
  return useMemo(
    () => (Result.isSuccess(result) ? indexOf(result.value.markers) : EMPTY),
    [result]
  );
};
