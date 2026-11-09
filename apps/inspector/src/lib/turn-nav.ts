/** Where `[` and `]` land in a transcript: the first item of each turn. */
import type { ChatNode, TranscriptRowRef } from "./chat-lane";
import type { HistorySort } from "./history-sort";

/** Indexes of the turn rules in a chat plan. */
export const chatTurnStops = (plan: readonly ChatNode[]) =>
  plan.flatMap((n, i) => (n.type === "turn" ? [i] : []));

/** Indexes of the table rows where a new turn starts. Turns only form runs in
 *  transcript order, so any other sort has no stops. */
export const tableTurnStops = ({
  rows,
  sort,
  turns,
}: {
  readonly rows: readonly TranscriptRowRef[];
  readonly sort: HistorySort;
  readonly turns: readonly number[];
}) => {
  if (sort.key !== "order") {
    return [];
  }
  let last: number | null = null;
  return rows.flatMap((row, i) => {
    const turn = turns[row.pos] ?? 0;
    const starts = turn !== last;
    last = turn;
    return starts ? [i] : [];
  });
};

/** The stop after (or before) `anchor`, or null at the end. An `anchor` of -1
 *  means the list starts below the top of the screen. */
export const stepStop = ({
  anchor,
  dir,
  stops,
}: {
  readonly anchor: number;
  readonly dir: "next" | "prev";
  readonly stops: readonly number[];
}) => {
  if (dir === "next") {
    return stops.find((s) => s > anchor) ?? null;
  }
  const before = stops.filter((s) => s < anchor);
  return before.at(-1) ?? null;
};
