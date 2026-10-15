/** A non-tool event body as the reader should see it: Markdown for text the
 *  agent or the harness wrote, a highlighted block for a file's content. */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import {
  CodeBlock,
  CodeBlockCopyButton,
} from "@workspace/ui/components/ai-elements/code-block";
import { Markdown } from "@workspace/ui/components/ai-elements/markdown";
import { cn } from "@workspace/ui/lib/utils";
import { bodyView, EMPTY_BODY } from "../../lib/tool-event";

const COPY = "absolute top-1 right-1 z-10 size-7";

/** Markdown with a copy button for its source in the corner. */
export const ProseBody = ({
  className,
  text,
}: {
  readonly className?: string;
  readonly text: string;
}) => (
  <div
    className={cn(
      "relative rounded-md border bg-background px-3 py-2 text-xs leading-relaxed",
      className
    )}
  >
    <CodeBlockCopyButton className={COPY} text={text} />
    <Markdown className="pr-8" text={text} />
  </div>
);

/** One event's body, or `empty` in italics when it has none. */
export const EventBody = ({
  className,
  e,
  empty = EMPTY_BODY,
}: {
  readonly className?: string;
  readonly e: TimelineEvent;
  readonly empty?: string;
}) => {
  if (e.body.trim().length === 0) {
    return (
      <p className={cn("text-muted-foreground italic", className)}>{empty}</p>
    );
  }
  const view = bodyView(e);
  return view.type === "prose" ? (
    <ProseBody className={className} text={e.body} />
  ) : (
    <CodeBlock
      className={className}
      code={e.body}
      language={view.language}
      wrap
    >
      <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
    </CodeBlock>
  );
};
