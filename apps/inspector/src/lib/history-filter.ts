/** Which events the history shows for a search and a type filter.
 *
 * Search runs on every keystroke over every event, so the lowercased text it
 * matches is built once per session load, not once per keystroke.
 */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { TranscriptRowRef } from "./chat-lane";

/** The text a search matches, per event position, already lowercased. */
export const searchIndex = (events: readonly TimelineEvent[]) =>
  events.map((e) => `${e.title} ${e.preview}`.toLowerCase());

/** Rows for one kind and query, in transcript order, each keeping its position
 *  in `events`. Harness `system` lines never show. */
export const filterRows = ({
  events,
  hay,
  kind,
  query,
}: {
  readonly events: readonly TimelineEvent[];
  /** `searchIndex(events)`. */
  readonly hay: readonly string[];
  readonly kind: string;
  readonly query: string;
}) => {
  const needle = query.toLowerCase();
  const out: TranscriptRowRef[] = [];
  for (const [pos, e] of events.entries()) {
    if (e.kind === "system" || (kind !== "all" && e.kind !== kind)) {
      continue;
    }
    if (needle.length > 0 && !hay[pos]?.includes(needle)) {
      continue;
    }
    out.push({ e, pos });
  }
  return out;
};

/** The table row that shows tool call `id`: the call itself, else any row
 *  with that id (its result, when a filter hides the call). -1 when none. */
export const rowIndexOfCall = (
  rows: readonly TranscriptRowRef[],
  id: string
) => {
  const call = rows.findIndex(
    ({ e }) => e.kind === "tool-call" && e.toolUseId === id
  );
  return call >= 0 ? call : rows.findIndex(({ e }) => e.toolUseId === id);
};
