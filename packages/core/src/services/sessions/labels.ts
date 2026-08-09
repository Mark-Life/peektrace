/** Human-facing labels for timeline events.
 *
 * Lives beside the parser rather than in either UI so the web inspector and the
 * terminal transcript badge the same event with the same words.
 */
import type { TimelineEvent } from "./schema";

/**
 * The badge label for one transcript row: the tool name when there is one, else
 * the attachment type as words, else the bare event kind.
 *
 * The type is humanised mechanically (`skill_listing` → `skill listing`) rather
 * than looked up, so a type Claude starts writing tomorrow reads as words on the
 * day it appears instead of collapsing to `attachment`.
 */
export const eventBadgeLabel = (e: TimelineEvent): string => {
  if (e.toolName !== undefined) {
    return e.toolName;
  }
  const type = e.attachmentType?.trim() ?? "";
  return type === "" ? e.kind : type.replaceAll("_", " ");
};
