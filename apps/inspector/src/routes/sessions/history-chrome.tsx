/** Static pieces of the history toolbar and banner. */
import { Button } from "@workspace/ui/components/button";
import { cn } from "@workspace/ui/lib/utils";
import { MessagesSquareIcon, ShieldAlertIcon, TableIcon } from "lucide-react";

import type { TranscriptMode } from "../../lib/transcript-mode";

/** Keeps the warning in sight once the banner has scrolled away. */
export const RedactionOffBadge = ({
  redacted,
}: {
  readonly redacted: boolean;
}) =>
  redacted ? null : (
    <span
      className="border-destructive/60 bg-destructive/15 text-destructive inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs font-medium"
      data-testid="sticky-redaction-off"
    >
      <ShieldAlertIcon className="size-3.5" />
      Redaction off
    </span>
  );

/** The persistent "review before sharing" notice above the transcript. */
export const RedactionBanner = ({
  redacted,
}: {
  readonly redacted: boolean;
}) => (
  <div
    className={cn(
      "text-destructive flex items-center gap-2 rounded-md border px-3 py-2 text-xs",
      redacted
        ? "border-destructive/30 bg-destructive/5"
        : "border-destructive/60 bg-destructive/15 font-medium"
    )}
    data-testid="redaction-banner"
  >
    <ShieldAlertIcon className="size-4 shrink-0" />
    {redacted ? (
      <span>
        Secrets are redacted by default. This transcript may still contain
        sensitive data, and images are shown as recorded — review before
        sharing.
      </span>
    ) : (
      <span data-testid="redaction-off">
        Redaction is OFF — the raw transcript (including secrets) is shown. Do
        not share.
      </span>
    )}
  </div>
);

/** Chat or table reading of the same rows. */
export const LayoutToggle = ({
  mode,
  onMode,
}: {
  readonly mode: TranscriptMode;
  readonly onMode: (mode: TranscriptMode) => void;
}) => (
  <fieldset
    aria-label="Transcript layout"
    className="border-border ml-auto flex items-center gap-0.5 rounded-md border p-0.5"
    data-testid="history-layout-toggle"
  >
    <Button
      data-testid="history-layout-chat"
      onClick={() => {
        onMode("chat");
      }}
      size="sm"
      variant={mode === "chat" ? "secondary" : "ghost"}
    >
      <MessagesSquareIcon className="size-3.5" />
      Chat
    </Button>
    <Button
      data-testid="history-layout-table"
      onClick={() => {
        onMode("table");
      }}
      size="sm"
      variant={mode === "table" ? "secondary" : "ghost"}
    >
      <TableIcon className="size-3.5" />
      Table
    </Button>
  </fieldset>
);
