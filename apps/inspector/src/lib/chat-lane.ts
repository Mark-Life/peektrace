/** Turning a filtered transcript into a chat script.
 *
 * The chat layout is a second reading of the same filtered rows the table gets,
 * so all of its shaping decisions — which side an event sits on, which call owns
 * which result, which neighbours collapse into one chip cluster — live here as
 * pure functions instead of inside JSX. The renderer then only maps nodes to
 * components, and a wrong lane or a lost result is a unit-test failure rather
 * than something you have to spot by eye in a 2000-event session.
 */
import { eventBadgeLabel } from "@workspace/core/services/sessions/labels";
import { isUserAuthored } from "@workspace/core/services/sessions/prompt-origin";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { HistoryRow } from "./api-types.gen";
import { chatCollapseId } from "./session-view";

/** Which column an event reads in: prompts right, model work left, context centred. */
export type ChatLane = "user" | "agent" | "center";

/** A whole event, or a history index row: the same fields without the text,
 *  plus the body's length and the first position with the same body. */
export type EventRow = TimelineEvent | HistoryRow;

/** What decides "same body": the body itself, or an index row's body id. */
export const bodyKey = (e: EventRow) => ("bodyId" in e ? e.bodyId : e.body);

/** `eventBadgeLabel` of a whole event or an index row: it reads only the
 *  kind, tool name, prompt origin and attachment type, which rows carry. */
export const badgeOf = (e: EventRow) => eventBadgeLabel(e as TimelineEvent);

/** Body length in UTF-16 units, as `body.length` counts it. */
export const bodyLength = (e: EventRow) =>
  "bodyLen" in e ? e.bodyLen : e.body.length;

/** A visible event plus its position in the unfiltered `a.events` — `pos` is what
 *  every collapse id is derived from, so it survives filtering and re-sorting. */
export interface TranscriptRowRef<E extends EventRow = TimelineEvent> {
  readonly e: E;
  readonly pos: number;
}

/** A chip of a context run. `repeat` is set when an earlier chip of the same
 *  identity already carried this exact body, so the chat can fold it. */
export interface RunItem<E extends EventRow = TimelineEvent>
  extends TranscriptRowRef<E> {
  readonly repeat: boolean;
}

/** One thing the chat renders: a bubble, a tool card, a chip cluster, a band, a
 *  centred notice from the harness, or a synthetic separator. */
export type ChatNode<E extends EventRow = TimelineEvent> =
  | { readonly type: "message"; readonly row: TranscriptRowRef<E> }
  | {
      readonly type: "tool";
      readonly call: TranscriptRowRef<E> | null;
      readonly result: TranscriptRowRef<E> | null;
    }
  | { readonly type: "run"; readonly items: readonly RunItem<E>[] }
  | { readonly type: "band"; readonly row: TranscriptRowRef<E> }
  | { readonly type: "notice"; readonly row: TranscriptRowRef<E> }
  | { readonly type: "turn"; readonly turn: number }
  | { readonly type: "dumbzone" };

/** Body length past which a bubble is clamped to a fixed height behind a fade.
 *  Counted in characters, not rendered lines, so one very long unwrapped line
 *  clamps the same as many short ones. */
export const CLAMP_CHARS = 800;

/** Attachment titles that already carry a file name after their type prefix. */
export const FILE_TITLE_PREFIXES = [
  "file: ",
  "edited_text_file: ",
  "opened_file_in_ide: ",
];

/** Which side of the conversation an event belongs to. A sidechain event is
 *  never the human talking, so it stays in the agent column whatever its kind;
 *  a prompt the harness wrote (task notification, wake-up) is centred. */
export const laneOf = (e: EventRow): ChatLane => {
  if (e.kind === "user-prompt") {
    if (e.isSidechain === true) {
      return "agent";
    }
    return isUserAuthored(e) ? "user" : "center";
  }
  if (
    e.kind === "assistant-text" ||
    e.kind === "assistant-thinking" ||
    e.kind === "tool-call" ||
    e.kind === "tool-result"
  ) {
    return "agent";
  }
  return "center";
};

/** The short label a context chip shows: the bare file name when the title is a
 *  file attachment (the parser has already reduced it to a basename), else the
 *  title, else the same badge word the table would print. */
export const chipLabel = (e: EventRow): string => {
  const prefix = FILE_TITLE_PREFIXES.find((p) => e.title.startsWith(p));
  if (prefix) {
    return e.title.slice(prefix.length);
  }
  return e.title.trim().length > 0 ? e.title : badgeOf(e);
};

/** Whether a bubble body is long enough to need the clamp + "Show all" affordance. */
export const needsClamp = (e: EventRow) => bodyLength(e) > CLAMP_CHARS;

/** The collapse id a node toggles, or null when the node is not interactive.
 *  A chip cluster has no id of its own — each chip inside it owns one. */
export const chatCollapseIdOf = (n: ChatNode<EventRow>): string | null => {
  if (n.type === "message" || n.type === "band" || n.type === "notice") {
    return chatCollapseId(n.row.pos);
  }
  if (n.type === "tool") {
    const pos = n.call?.pos ?? n.result?.pos;
    return pos === undefined ? null : chatCollapseId(pos);
  }
  return null;
};

/** Whether an event joins a centred chip cluster rather than standing alone.
 *  Harness `system` lines (hook summaries, turn durations) are bookkeeping, so
 *  they fold in with the context chips instead of reading as a reply. */
const isChip = (e: EventRow) =>
  e.kind === "attachment" || e.kind === "meta" || e.kind === "system";

/** What makes two chips "the same chip": kind, attachment type, label, and
 *  whether it ran inside a subagent. The body decides if it also repeats. */
const chipIdentity = (e: EventRow) =>
  `${e.kind}|${e.attachmentType ?? ""}|${chipLabel(e)}|${e.isSidechain === true}`;

/** The result that answers `call`, searched forward among the still-visible rows
 *  so a result the filter dropped simply leaves the call unanswered. */
const resultFor = <E extends EventRow>(
  rows: readonly TranscriptRowRef<E>[],
  from: number,
  call: TranscriptRowRef<E>
) => {
  const id = call.e.toolUseId;
  if (!id) {
    return null;
  }
  for (let i = from; i < rows.length; i++) {
    const row = rows[i];
    if (row && row.e.kind === "tool-result" && row.e.toolUseId === id) {
      return row;
    }
  }
  return null;
};

/** The node one standalone row becomes, plus the result it swallowed. */
const nodeForRow = <E extends EventRow>(
  rows: readonly TranscriptRowRef<E>[],
  i: number,
  row: TranscriptRowRef<E>
): { readonly consumed: number | null; readonly node: ChatNode<E> } => {
  const { e } = row;
  if (e.kind === "tool-call") {
    const result = resultFor(rows, i + 1, row);
    return {
      consumed: result?.pos ?? null,
      node: { type: "tool", call: row, result },
    };
  }
  if (e.kind === "tool-result") {
    return { consumed: null, node: { type: "tool", call: null, result: row } };
  }
  if (
    e.kind === "compaction" ||
    e.kind === "summary" ||
    e.kind === "system-prompt"
  ) {
    return { consumed: null, node: { type: "band", row } };
  }
  if (laneOf(e) === "center") {
    return { consumed: null, node: { type: "notice", row } };
  }
  return { consumed: null, node: { type: "message", row } };
};

/**
 * One forward pass turning visible rows into chat nodes: pair every tool call
 * with its result, group adjacent context injections into one cluster, and drop
 * in the synthetic turn rules and the dumb-zone band at their crossings.
 */
export const buildChatPlan = <E extends EventRow>({
  crossEvtIdx,
  rows,
  turns,
}: {
  readonly crossEvtIdx: number;
  readonly rows: readonly TranscriptRowRef<E>[];
  readonly turns: readonly number[];
}): readonly ChatNode<E>[] => {
  const nodes: ChatNode<E>[] = [];
  const consumed = new Set<number>();
  const run: RunItem<E>[] = [];
  const lastBody = new Map<string, number | string>();
  let lastTurn = 0;

  const flush = () => {
    if (run.length > 0) {
      nodes.push({ type: "run", items: [...run] });
      run.length = 0;
    }
  };

  for (const [i, row] of rows.entries()) {
    const { e, pos } = row;
    if (consumed.has(pos)) {
      continue;
    }
    if (crossEvtIdx >= 0 && pos === crossEvtIdx) {
      flush();
      nodes.push({ type: "dumbzone" });
    }
    const turn = turns[pos] ?? 0;
    if (turn > 0 && turn !== lastTurn) {
      flush();
      nodes.push({ type: "turn", turn });
      lastTurn = turn;
    }
    if (isChip(e)) {
      const identity = chipIdentity(e);
      const isImage = e.attachmentType === "image" || e.images !== undefined;
      run.push({
        ...row,
        repeat: !isImage && lastBody.get(identity) === bodyKey(e),
      });
      lastBody.set(identity, bodyKey(e));
      continue;
    }
    flush();
    const next = nodeForRow(rows, i, row);
    if (next.consumed !== null) {
      consumed.add(next.consumed);
    }
    nodes.push(next.node);
  }
  flush();
  return nodes;
};

/** Every collapse id the chat renders, in order — one per node, so a result
 *  folded into its call's card does not contribute a second, unreachable id
 *  that would keep "Expand all" from ever reading as fully open. */
export const chatIdsOf = (
  rows: readonly TranscriptRowRef<EventRow>[]
): readonly string[] => {
  const consumed = new Set<number>();
  for (const [i, row] of rows.entries()) {
    if (row.e.kind !== "tool-call") {
      continue;
    }
    const result = resultFor(rows, i + 1, row);
    if (result) {
      consumed.add(result.pos);
    }
  }
  return rows
    .filter(({ pos }) => !consumed.has(pos))
    .map(({ pos }) => chatCollapseId(pos));
};

/** The node that shows tool call `id`: its card, whether the call itself or
 *  only its result is visible. -1 when no visible node carries it. */
export const planIndexOfCall = (
  plan: readonly ChatNode<EventRow>[],
  id: string
) =>
  plan.findIndex(
    (n) =>
      n.type === "tool" &&
      (n.call?.e.toolUseId === id ||
        (n.call === null && n.result?.e.toolUseId === id))
  );

/** The key one node is known by, before deduplication. Built from event
 *  positions and turn numbers, never list positions, so a node keeps its key
 *  (and its measured height) when a filter or a live append moves it. */
const baseKeyOf = (n: ChatNode<EventRow>) => {
  if (n.type === "message" || n.type === "band" || n.type === "notice") {
    return `n${n.row.pos}`;
  }
  if (n.type === "tool") {
    return `n${n.call?.pos ?? n.result?.pos ?? "?"}`;
  }
  if (n.type === "run") {
    return `run${n.items[0]?.pos ?? "?"}`;
  }
  return n.type === "turn" ? `turn${n.turn}` : "dumbzone";
};

/** One unique key per node. A turn whose events are interleaved with another's
 *  draws its rule twice; the repeat gets a numbered suffix. */
export const chatNodeKeys = (plan: readonly ChatNode<EventRow>[]) => {
  const seen = new Map<string, number>();
  return plan.map((n) => {
    const base = baseKeyOf(n);
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    return count === 0 ? base : `${base}~${count}`;
  });
};
