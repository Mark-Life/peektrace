/** First-guess heights for transcript blocks that have not been drawn yet.
 *
 * A virtualized transcript only measures what it mounts; everything else is
 * placed by these numbers. They only need to be close: a good guess keeps the
 * scrollbar honest and the page from jumping when a block is finally measured.
 * Heights are in CSS pixels and include the gap below each chat block.
 */
import { type ChatNode, chatCollapseIdOf } from "./chat-lane";

/** Space below each chat block, as a class on each chat item and in pixels. */
export const CHAT_ITEM_CLASS = "pb-3";
export const CHAT_GAP = 12;

/** Characters per wrapped line in the chat column, and the line height. */
const CHARS_PER_LINE = 90;
const LINE = 20;

/** `max-h-48`, the clamp a long collapsed bubble sits behind. */
const CLAMPED_BODY = 192;
/** `max-h-[28rem]`, the cap on an opened bubble, band or notice body. */
const OPENED_BODY = 448;
/** `max-h-96`, the cap on an opened tool card or table row. */
const OPENED_TOOL = 384;

const HEADER = {
  band: 56,
  dumbzone: 32,
  message: 40,
  notice: 56,
  run: 40,
  tool: 44,
  turn: 28,
} as const satisfies Record<ChatNode["type"], number>;

/** Height of `chars` of wrapped text, capped at `max`. */
const textHeight = (chars: number, max: number) =>
  Math.min(Math.ceil(chars / CHARS_PER_LINE) * LINE, max);

/** What one opened chat node adds below its header. */
const openedBody = (n: ChatNode) => {
  if (n.type === "tool") {
    return OPENED_TOOL;
  }
  if (n.type === "message" || n.type === "band" || n.type === "notice") {
    return textHeight(n.row.e.body.length, OPENED_BODY);
  }
  return n.type === "run" ? OPENED_BODY : 0;
};

/** What a collapsed message shows: its body, up to the clamp. Thinking stays
 *  folded to one line until opened. */
const collapsedBody = (n: ChatNode) =>
  n.type === "message" && n.row.e.kind !== "assistant-thinking"
    ? textHeight(n.row.e.body.length, CLAMPED_BODY)
    : 0;

/** Guessed height of one chat node. A chip cluster has no id of its own, so it
 *  reads as open only when everything is. */
export const estimateChatNode = ({
  allOpen,
  isOpen,
  n,
}: {
  readonly allOpen: boolean;
  readonly isOpen: (id: string) => boolean;
  readonly n: ChatNode;
}) => {
  const id = chatCollapseIdOf(n);
  const open = id === null ? allOpen && n.type === "run" : isOpen(id);
  return HEADER[n.type] + (open ? openedBody(n) : collapsedBody(n)) + CHAT_GAP;
};

const TABLE_ROW = 33;
const TABLE_DIVIDER = 30;

/** Guessed height of one table row, with the dumb-zone divider when it sits
 *  above this row. */
export const estimateTableRow = ({
  divider,
  open,
}: {
  readonly divider: boolean;
  readonly open: boolean;
}) => TABLE_ROW + (open ? OPENED_TOOL : 0) + (divider ? TABLE_DIVIDER : 0);
