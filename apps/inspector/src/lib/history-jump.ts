/** Jumps into the transcript: from the session summary and from stats links.
 *
 * The summary chips and the growth timeline ask for a place (a turn, the first
 * compaction, the first subagent); a stats link names a call (`call=<id>`).
 * Here a request becomes a concrete target the transcript can find, and each
 * layout finds it in its own list. A turn is kept in the URL as `turn=N`, so
 * the place survives a reload; a new request clears whichever param no longer
 * holds.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import type { SessionJump } from "@workspace/viz/lib/session-jump";
import { type RefObject, useCallback, useRef, useState } from "react";
import type { ChatNode, TranscriptRowRef } from "./chat-lane";
import { planIndexOfCall } from "./chat-lane";
import { rowIndexOfCall } from "./history-filter";
import { readHashParam, setHashParams, useHashParam } from "./routes";

/** What asks for a jump: the summary, or a stats link naming a call. */
export type JumpRequest =
  | SessionJump
  | { readonly kind: "call"; readonly id: string };

/** A place in the transcript a jump lands on. */
type JumpTarget =
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

/** A turn the session has; the first compaction event, or else the turn the
 *  first compaction hit; a call that is in this transcript; the first subagent
 *  spawned in transcript order, or else the subagents section. Null when the
 *  session has no such place. */
export const resolveJump = (
  a: Pick<
    AnalyzedSession,
    "compactionTurns" | "events" | "subagents" | "turnCount"
  >,
  req: JumpRequest
): JumpTarget | null => {
  if (req.kind === "turn") {
    return req.turn >= 1 && req.turn <= a.turnCount
      ? { kind: "turn", turn: req.turn }
      : null;
  }
  if (req.kind === "call") {
    return a.events.some((e) => e.toolUseId === req.id)
      ? { kind: "call", id: req.id }
      : null;
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
  const ids = new Set(a.subagents.map((s) => s.toolUseId));
  const spawn = a.events.find(
    (e) => e.toolUseId !== undefined && ids.has(e.toolUseId)
  );
  return spawn?.toolUseId === undefined
    ? { kind: "subagents" }
    : { kind: "call", id: spawn.toolUseId };
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
const planIndexOfTurn = (plan: readonly ChatNode[], n: number) => {
  const exact = plan.findIndex((x) => x.type === "turn" && x.turn === n);
  return exact >= 0
    ? exact
    : plan.findIndex((x) => x.type === "turn" && x.turn >= n);
};

/** The band of the first visible compaction. */
const planIndexOfCompaction = (plan: readonly ChatNode[]) =>
  plan.findIndex((x) => x.type === "band" && x.row.e.kind === "compaction");

/** The first row of turn `n`. The later-turn fallback only holds in forward
 *  transcript order (`ordered`); under a column sort, "later" means nothing. */
const rowIndexOfTurn = ({
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
const rowIndexOfCompaction = (rows: readonly TranscriptRowRef[]) =>
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

/** A jump not landed yet whose target the session has but the search or type
 *  filter hides (`index`, its place in the shown list, is -1). `resolveJump`
 *  already dropped places the session lacks. The subagents section is outside
 *  the filtered list, so it is never hidden. */
export const isHiddenJump = ({
  filtered,
  index,
  jump,
  landed,
}: {
  /** A search or a type filter is on. */
  readonly filtered: boolean;
  readonly index: number;
  readonly jump: HistoryJump | null;
  /** The key of the jump that last landed. */
  readonly landed: string | null;
}) =>
  jump !== null &&
  jump.key !== landed &&
  jump.target.kind !== "subagents" &&
  filtered &&
  index < 0;

/** A turn lands with its rule at the top; a call card is centred. */
export const alignOf = (target: JumpTarget) =>
  target.kind === "call" ? "center" : "start";

/** A positive integer from the `turn` URL param, or null. */
const parseTurnParam = (raw: string | null) => {
  const n = raw === null ? Number.NaN : Number(raw);
  return Number.isInteger(n) && n >= 1 ? n : null;
};

/** The request a page load starts with: `turn=` wins over `call=`, since a
 *  turn is only written by a later jump. */
export const seedRequest = ({
  call,
  turn,
}: {
  readonly call: string | null;
  readonly turn: string | null;
}): JumpRequest | null => {
  const n = parseTurnParam(turn);
  if (n !== null) {
    return { kind: "turn", turn: n };
  }
  return call === null ? null : { kind: "call", id: call };
};

/** How far a jump got. Held above the history, so a remount (a layout switch,
 *  the redaction re-fetch) does not land the same jump again. */
export interface JumpLanding {
  /** The key of the jump in flight or landed. */
  readonly done: RefObject<string | null>;
  /** The key of the jump that last landed. */
  readonly landed: string | null;
  readonly onLand: (key: string) => void;
}

/** The current jump request, seeded from the URL, and how far it got. A stats
 *  link that changes `call=` asks again; `request` asks for a summary place. */
export const useHistoryJump = () => {
  const call = useHashParam("call");
  const [state, setState] = useState(() => ({
    call,
    req: seedRequest({ call, turn: readHashParam("turn") }),
    seq: 0,
  }));
  if (call !== state.call) {
    // Cleared by `request` below, or a new link into this session.
    setState(
      call === null
        ? { ...state, call }
        : { call, req: { kind: "call", id: call }, seq: state.seq + 1 }
    );
  }
  const request = useCallback((req: SessionJump) => {
    setHashParams({
      call: null,
      turn: req.kind === "turn" ? String(req.turn) : null,
    });
    setState((prev) => ({ ...prev, req, seq: prev.seq + 1 }));
  }, []);
  const done = useRef<string | null>(null);
  const [landed, setLanded] = useState<string | null>(null);
  const landing: JumpLanding = { done, landed, onLand: setLanded };
  return { landing, req: state.req, request, seq: state.seq };
};
