/** Locating a tool call among the history rows a filter left visible. */
import type { TranscriptRowRef } from "./chat-lane";

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
