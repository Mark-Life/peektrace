/** Human-facing labels for timeline events.
 *
 * Lives beside the parser rather than in either UI so the web inspector and the
 * terminal transcript badge the same event with the same words.
 */
import type { TimelineEvent } from "./schema";

/**
 * The badge label for one transcript row: the tool name when there is one, else
 * "system prompt" for system prompts, else the attachment type as words, else
 * the bare event kind.
 *
 * The type is humanised mechanically (`skill_listing` → `skill listing`) rather
 * than looked up, so a type Claude starts writing tomorrow reads as words on the
 * day it appears instead of collapsing to `attachment`.
 */
export const eventBadgeLabel = (e: TimelineEvent): string => {
  if (e.toolName !== undefined) {
    return e.toolName;
  }
  if (e.kind === "system-prompt") {
    return "system prompt";
  }
  const type = e.attachmentType?.trim() ?? "";
  return type === "" ? e.kind : type.replaceAll("_", " ");
};

/** Short tag for an event a later context edit changed, else undefined. */
export const contextEditTag = (e: TimelineEvent) => {
  switch (e.contextEdit?.status) {
    case "removed":
      return "removed";
    case "replaced":
      return "edited";
    default:
      return;
  }
};

/** One-paragraph explanation of a context edit, with the original text when kept. */
export const contextEditNote = (e: TimelineEvent) => {
  const edit = e.contextEdit;
  if (!edit) {
    return;
  }
  const lines = [...(edit.steps ?? []), edit].map((s) => s.atIndex + 1);
  const first = lines[0];
  const earlier =
    lines.length > 1
      ? ` Earlier edit${lines.length > 2 ? "s" : ""} at line${lines.length > 2 ? "s" : ""} ${lines.slice(0, -1).join(", ")}.`
      : "";
  if (edit.status === "removed") {
    return `Dropped from model context by the edit at line ${edit.atIndex + 1}.${earlier} Later turns do not see it.`;
  }
  const head = `Edited in model context at line ${edit.atIndex + 1}.${earlier} Turns before line ${first} saw the original`;
  return edit.original === undefined
    ? `${head}.`
    : `${head} (~${edit.originalTokensEst} tok):\n\n${edit.original}`;
};
