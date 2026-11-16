/** Marks for an event a later context edit changed: a header badge and a note
 *  with the original text. The Rust core keeps the same wording for the TUI. */
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import type { TimelineEvent } from "@workspace/view-core/api-types.gen";

import { contextEditNote, contextEditTag } from "../../lib/event-labels";

/** `removed` / `edited` badge, or nothing for an untouched event. */
export const ContextEditBadge = ({
  e,
  className,
}: {
  readonly e: TimelineEvent | null | undefined;
  readonly className?: string;
}) => {
  const tag = e ? contextEditTag(e) : undefined;
  if (!(e && tag)) {
    return null;
  }
  return (
    <Badge
      className={cn(
        "shrink-0 border-amber-500/40 bg-amber-500/10 text-amber-400",
        className
      )}
      data-testid="context-edit-badge"
      title={contextEditNote(e)?.split("\n")[0]}
      variant="outline"
    >
      {tag}
    </Badge>
  );
};

/** The edit explanation, quoting the original content when the transcript has it. */
export const ContextEditNote = ({
  e,
}: {
  readonly e: TimelineEvent | null | undefined;
}) => {
  const note = e ? contextEditNote(e) : undefined;
  if (note === undefined) {
    return null;
  }
  return (
    <pre
      className="text-muted-foreground max-h-64 overflow-auto border-l-2 border-amber-500/40 bg-amber-500/5 px-3 py-2 text-xs wrap-break-word whitespace-pre-wrap"
      data-testid="context-edit-note"
    >
      {note}
    </pre>
  );
};
