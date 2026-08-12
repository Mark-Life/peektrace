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
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import { chatCollapseId } from "./session-view";

/** Which column an event reads in: prompts right, model work left, context centred. */
export type ChatLane = "user" | "agent" | "center";

/** A visible event plus its position in the unfiltered `a.events` — `pos` is what
 *  every collapse id is derived from, so it survives filtering and re-sorting. */
export interface TranscriptRowRef {
  readonly e: TimelineEvent;
  readonly pos: number;
}

/** One thing the chat renders: a bubble, a tool card, a chip cluster, a band, or
 *  a synthetic separator. */
export type ChatNode =
  | { readonly type: "message"; readonly row: TranscriptRowRef }
  | {
      readonly type: "tool";
      readonly call: TranscriptRowRef | null;
      readonly result: TranscriptRowRef | null;
    }
  | { readonly type: "run"; readonly items: readonly TranscriptRowRef[] }
  | { readonly type: "band"; readonly row: TranscriptRowRef }
  | { readonly type: "turn"; readonly turn: number }
  | { readonly type: "dumbzone" };

/** Body length past which a bubble is clamped to a fixed height behind a fade.
 *  Counted in characters, not rendered lines, so one very long unwrapped line
 *  clamps the same as many short ones. */
export const CLAMP_CHARS = 800;

/** Attachment titles that already carry a file name after their type prefix. */
const FILE_TITLE_PREFIXES = [
  "file: ",
  "edited_text_file: ",
  "opened_file_in_ide: ",
];

/** Which side of the conversation an event belongs to. A sidechain event is
 *  never the human talking, so it stays in the agent column whatever its kind. */
export const laneOf = (e: TimelineEvent): ChatLane => {
  if (e.kind === "user-prompt") {
    return e.isSidechain === true ? "agent" : "user";
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
export const chipLabel = (e: TimelineEvent): string => {
  const prefix = FILE_TITLE_PREFIXES.find((p) => e.title.startsWith(p));
  if (prefix) {
    return e.title.slice(prefix.length);
  }
  return e.title.trim().length > 0 ? e.title : eventBadgeLabel(e);
};

/** Whether a bubble body is long enough to need the clamp + "Show all" affordance. */
export const needsClamp = (e: TimelineEvent) => e.body.length > CLAMP_CHARS;

/** The collapse id a node toggles, or null when the node is not interactive.
 *  A chip cluster has no id of its own — each chip inside it owns one. */
export const chatCollapseIdOf = (n: ChatNode): string | null => {
  if (n.type === "message" || n.type === "band") {
    return chatCollapseId(n.row.pos);
  }
  if (n.type === "tool") {
    const pos = n.call?.pos ?? n.result?.pos;
    return pos === undefined ? null : chatCollapseId(pos);
  }
  return null;
};

/** Whether an event joins a centred chip cluster rather than standing alone. */
const isChip = (e: TimelineEvent) =>
  e.kind === "attachment" || e.kind === "meta";

/** The result that answers `call`, searched forward among the still-visible rows
 *  so a result the filter dropped simply leaves the call unanswered. */
const resultFor = (
  rows: readonly TranscriptRowRef[],
  from: number,
  call: TranscriptRowRef
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
const nodeForRow = (
  rows: readonly TranscriptRowRef[],
  i: number,
  row: TranscriptRowRef
): { readonly consumed: number | null; readonly node: ChatNode } => {
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
  if (e.kind === "compaction" || e.kind === "summary") {
    return { consumed: null, node: { type: "band", row } };
  }
  return { consumed: null, node: { type: "message", row } };
};

/**
 * One forward pass turning visible rows into chat nodes: pair every tool call
 * with its result, group adjacent context injections into one cluster, and drop
 * in the synthetic turn rules and the dumb-zone band at their crossings.
 */
export const buildChatPlan = ({
  crossEvtIdx,
  rows,
  turns,
}: {
  readonly crossEvtIdx: number;
  readonly rows: readonly TranscriptRowRef[];
  readonly turns: readonly number[];
}): readonly ChatNode[] => {
  const nodes: ChatNode[] = [];
  const consumed = new Set<number>();
  const run: TranscriptRowRef[] = [];
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
      run.push(row);
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
  rows: readonly TranscriptRowRef[]
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
