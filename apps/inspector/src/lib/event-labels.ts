/** Human-facing labels for timeline events. The Rust core keeps a port of
 * these (`sessions::labels`) for the TUI and session export.
 */
import type {
  ContextEdit,
  PromptOrigin,
  TimelineEvent,
} from "@workspace/view-core/api-types.gen";

/** The event fields the badge reads; index rows carry them too. */
export type BadgeSource = Pick<
  TimelineEvent,
  "attachmentType" | "kind" | "promptOrigin" | "toolName"
>;

/**
 * The badge label for one transcript row: the tool name when there is one, else
 * "system prompt" for system prompts, else what wrote a prompt the user did not
 * type (`task notification`, `scheduled`, …), else the attachment type as
 * words, else the bare event kind.
 *
 * The type is humanised mechanically (`skill_listing` → `skill listing`) rather
 * than looked up, so a type Claude starts writing tomorrow reads as words on the
 * day it appears instead of collapsing to `attachment`.
 */
export const eventBadgeLabel = (e: BadgeSource): string => {
  if (e.toolName !== undefined) {
    return e.toolName;
  }

  if (e.kind === "system-prompt") {
    return "system prompt";
  }

  if (
    e.kind === "user-prompt" &&
    e.promptOrigin !== undefined &&
    e.promptOrigin !== "human" &&
    e.promptOrigin !== "sdk"
  ) {
    return e.promptOrigin.replaceAll("-", " ");
  }

  const type = e.attachmentType?.trim() ?? "";

  return type === "" ? e.kind : type.replaceAll("_", " ");
};

const CONTEXT_EDIT_TAGS = { removed: "removed", replaced: "edited" } as const;

/** Short tag for an event a later context edit changed, else undefined. */
export const contextEditTag = (e: TimelineEvent) =>
  e.contextEdit === undefined
    ? undefined
    : CONTEXT_EDIT_TAGS[e.contextEdit.status];

const MULTIPLE_EDITS = 2;

const describeContextEdit = (edit: ContextEdit) => {
  const lines = [...(edit.steps ?? []), edit].map((s) => s.atIndex + 1);
  const [first] = lines;

  const plural = lines.length > MULTIPLE_EDITS ? "s" : "";

  const earlier =
    lines.length > 1
      ? ` Earlier edit${plural} at line${plural} ${lines.slice(0, -1).join(", ")}.`
      : "";

  if (edit.status === "removed") {
    return `Dropped from model context by the edit at line ${edit.atIndex + 1}.${earlier} Later turns do not see it.`;
  }

  const head = `Edited in model context at line ${edit.atIndex + 1}.${earlier} Turns before line ${first} saw the original`;

  return edit.original === undefined
    ? `${head}.`
    : `${head} (~${edit.originalTokensEst} tok):\n\n${edit.original}`;
};

/** One-paragraph explanation of a context edit, with the original text when kept. */
export const contextEditNote = (e: TimelineEvent) =>
  e.contextEdit === undefined ? undefined : describeContextEdit(e.contextEdit);

const MS_PER_SECOND = 1000;
const SHORT_SECONDS = 10;

/** Thinking time as a short label: `850ms`, `4.2s`, `73s`. */
export const fmtThinkingMs = (ms: number) => {
  if (ms < MS_PER_SECOND) {
    return `${Math.round(ms)}ms`;
  }

  const s = ms / MS_PER_SECOND;

  return s < SHORT_SECONDS ? `${s.toFixed(1)}s` : `${Math.round(s)}s`;
};

/** What to say about a thinking block whose text the transcript did not keep. */
export const thinkingNote = (e: TimelineEvent) =>
  e.tokensMeasured === undefined
    ? "Thinking content is not stored in the transcript (only a signature). Its token cost is in the timeline 'thinking' band."
    : `Thinking text is not stored in the transcript (only a signature). Usage reports ${e.tokensMeasured} thinking tokens for it; they stay in the context window on later turns.`;

/** Origins a person sent: they read on the user's side of the chat. */
const FROM_USER: ReadonlySet<PromptOrigin> = new Set([
  "human",
  "sdk",
  "command",
]);

/** Whether a prompt came from the user (typed, sent through the SDK, or a
 *  slash command) rather than from the harness. Unclassified prompts (agents
 *  whose transcripts carry no origin) count as the user's. */
export const isUserAuthored = (
  e: Pick<TimelineEvent, "kind" | "promptOrigin">
) =>
  e.kind === "user-prompt" &&
  (e.promptOrigin === undefined || FROM_USER.has(e.promptOrigin));
