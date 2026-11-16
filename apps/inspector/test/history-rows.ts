/** Index rows built from whole events, as `/history/index` serves them. */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { HistoryRow } from "../src/lib/api-types.gen";

export const indexRowsOf = (
  events: readonly TimelineEvent[],
  turns: readonly number[]
): HistoryRow[] =>
  events.map((e, pos) => {
    const {
      body,
      contextEdit,
      preview: _p,
      requestId: _r,
      ts: _t,
      ...rest
    } = e;
    const edit =
      contextEdit === undefined
        ? {}
        : { contextEdit: { ...contextEdit, original: undefined } };
    return {
      ...rest,
      ...edit,
      bodyId: events.findIndex((x) => x.body === body),
      bodyLen: body.length,
      turn: turns[pos] ?? 0,
    };
  });
