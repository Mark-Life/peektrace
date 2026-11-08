/** Jumps from the session summary into the transcript.
 *
 * The summary chips and the growth timeline ask for a place (a turn, the first
 * compaction, the first subagent). Here that request becomes a concrete target
 * the transcript can find, and each layout finds it in its own list. A turn is
 * also kept in the URL as `turn=N`, so the place survives a reload.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import type { SessionJump } from "@workspace/viz/lib/session-jump";
import { useCallback, useState } from "react";
import type { ChatNode, TranscriptRowRef } from "./chat-lane";
import { planIndexOfCall } from "./chat-lane";
import { rowIndexOfCall } from "./history-filter";
import type { HistorySort } from "./history-sort";
import { setHashParam, useHashParam } from "./routes";

/** A place in the transcript a jump lands on. */
export type JumpTarget =
  | { readonly kind: "turn"; readonly turn: number }
  | { readonly kind: "compaction" }
  | { readonly kind: "call"; readonly id: string }
  | { readonly kind: "subagents" };

/** A resolved jump: where to go, and a key that changes on every request, so a
 *  second click on the same chip lands again. */
export interface HistoryJump {
  readonly key: string;
  readonly target: JumpTarget;
}

/** The first compaction event, or else the turn the first compaction hit; the
 *  first subagent's spawning call when it is in this transcript, or else the
 *  subagents section. Null when the session has no such place. */
export const resolveJump = (
  a: Pick<AnalyzedSession, "compactionTurns" | "events" | "subagents">,
  req: SessionJump
): JumpTarget | null => {
  if (req.kind === "turn") {
    return req.turn >= 1 ? { kind: "turn", turn: req.turn } : null;
  }
  if (req.kind === "compaction") {
    if (a.events.some((e) => e.kind === "compaction")) {
      return { kind: "compaction" };
    }
    const ti = a.compactionTurns[0];
    return ti === undefined ? null : { kind: "turn", turn: ti + 1 };
  }
  if (a.subagents.length === 0) {
    return null;
  }
  const ids = new Set(a.events.map((e) => e.toolUseId));
  const spawned = a.subagents.find(
    (s) => s.toolUseId !== undefined && ids.has(s.toolUseId)
  );
  return spawned?.toolUseId === undefined
    ? { kind: "subagents" }
    : { kind: "call", id: spawned.toolUseId };
};

/** The key one request is known by. */
export const jumpKey = (target: JumpTarget, seq: number) => {
  if (target.kind === "turn") {
    return `turn:${target.turn}@${seq}`;
  }
  return target.kind === "call"
    ? `call:${target.id}@${seq}`
    : `${target.kind}@${seq}`;
};

/** The turn rule of turn `n`, or the first later one when a filter dropped
 *  every event of `n`. -1 when nothing at or after `n` is shown. */
export const planIndexOfTurn = (plan: readonly ChatNode[], n: number) => {
  const exact = plan.findIndex((x) => x.type === "turn" && x.turn === n);
  return exact >= 0
    ? exact
    : plan.findIndex((x) => x.type === "turn" && x.turn >= n);
};

/** The band of the first visible compaction. */
export const planIndexOfCompaction = (plan: readonly ChatNode[]) =>
  plan.findIndex((x) => x.type === "band" && x.row.e.kind === "compaction");

/** The first row of turn `n`. The later-turn fallback only holds in forward
 *  transcript order (`ordered`); under a column sort, "later" means nothing. */
export const rowIndexOfTurn = ({
  n,
  ordered,
  rows,
  turns,
}: {
  readonly n: number;
  readonly ordered: boolean;
  readonly rows: readonly TranscriptRowRef[];
  readonly turns: readonly number[];
}) => {
  const exact = rows.findIndex(({ pos }) => turns[pos] === n);
  if (exact >= 0 || !ordered) {
    return exact;
  }
  return rows.findIndex(({ pos }) => (turns[pos] ?? 0) >= n);
};

/** The first visible compaction row. */
export const rowIndexOfCompaction = (rows: readonly TranscriptRowRef[]) =>
  rows.findIndex(({ e }) => e.kind === "compaction");

/** Where `target` sits in the chat plan; -1 when hidden or not in the list. */
export const planIndexOfTarget = (
  plan: readonly ChatNode[],
  target: JumpTarget
) => {
  if (target.kind === "turn") {
    return planIndexOfTurn(plan, target.turn);
  }
  if (target.kind === "compaction") {
    return planIndexOfCompaction(plan);
  }
  return target.kind === "call" ? planIndexOfCall(plan, target.id) : -1;
};

/** Where `target` sits in the table rows; -1 when hidden or not in the list. */
export const rowIndexOfTarget = ({
  ordered,
  rows,
  target,
  turns,
}: {
  readonly ordered: boolean;
  readonly rows: readonly TranscriptRowRef[];
  readonly target: JumpTarget;
  readonly turns: readonly number[];
}) => {
  if (target.kind === "turn") {
    return rowIndexOfTurn({ n: target.turn, ordered, rows, turns });
  }
  if (target.kind === "compaction") {
    return rowIndexOfCompaction(rows);
  }
  return target.kind === "call" ? rowIndexOfCall(rows, target.id) : -1;
};

/** A jump not landed yet whose target the session has but the current
 *  filters hide. The subagents section is outside the filtered list, so it is
 *  never hidden. */
export const isHiddenJump = ({
  all,
  jump,
  landed,
  sort,
  turns,
  visible,
}: {
  readonly all: readonly TranscriptRowRef[];
  readonly jump: HistoryJump | null;
  /** The key of the jump that last landed. */
  readonly landed: string | null;
  /** The sort the rows are in; "later turn" fallbacks need forward order. */
  readonly sort: HistorySort;
  readonly turns: readonly number[];
  readonly visible: readonly TranscriptRowRef[];
}) => {
  if (
    jump === null ||
    jump.key === landed ||
    jump.target.kind === "subagents"
  ) {
    return false;
  }
  const { target } = jump;
  const ordered = sort.key === "order" && sort.dir === "asc";
  return (
    rowIndexOfTarget({ ordered, rows: visible, target, turns }) < 0 &&
    rowIndexOfTarget({ ordered, rows: all, target, turns }) >= 0
  );
};

/** A turn lands with its rule at the top; a call card is centred. */
export const alignOf = (target: JumpTarget) =>
  target.kind === "call" ? "center" : "start";

/** A positive integer from the `turn` URL param, or null. */
export const parseTurnParam = (raw: string | null) => {
  const n = raw === null ? Number.NaN : Number(raw);
  return Number.isInteger(n) && n >= 1 ? n : null;
};

/** The current jump request, seeded from `turn=` in the URL. `request` asks
 *  for a new one; a turn is written back to the URL. */
export const useHistoryJump = () => {
  const turnParam = useHashParam("turn");
  const [state, setState] = useState<{
    readonly req: SessionJump | null;
    readonly seq: number;
  }>(() => {
    const turn = parseTurnParam(turnParam);
    return { req: turn === null ? null : { kind: "turn", turn }, seq: 0 };
  });
  const request = useCallback((req: SessionJump) => {
    if (req.kind === "turn") {
      setHashParam("turn", String(req.turn));
    }
    setState((prev) => ({ req, seq: prev.seq + 1 }));
  }, []);
  return { ...state, request };
};
