/** Download the session as HTML, Markdown or JSON. The detail holds only
 * parts of the analysis, so a click fetches the whole one first, with the
 * same redaction as the screen. */
import {
  EXPORT_FORMATS,
  type ExportFormat,
} from "@workspace/core/services/sessions/export/index";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Button } from "@workspace/ui/components/button";
import { Effect } from "effect";
import { DownloadIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { apiGet, apiPath } from "../../lib/api";
import { exportFile } from "../../lib/export-file";
import type { SessionKey } from "../../lib/session-data";
import { wireErrorMessage } from "../../lib/wire-error";

const download = (
  a: AnalyzedSession,
  format: ExportFormat,
  redacted: boolean
) => {
  const file = exportFile(a, format, redacted);
  const url = URL.createObjectURL(new Blob([file.text], { type: file.type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = file.name;
  link.click();
  URL.revokeObjectURL(url);
};

const analysisOf = (k: SessionKey) =>
  apiGet<AnalyzedSession>(apiPath("sessions", k.id, "analysis"), {
    parent: k.parent ?? undefined,
    redact: k.redact ? undefined : "false",
  });

export const ExportButtons = ({ k }: { readonly k: SessionKey }) => {
  const [busy, setBusy] = useState<ExportFormat | null>(null);
  const run = (format: ExportFormat) => {
    setBusy(format);
    Effect.runPromise(Effect.either(analysisOf(k)))
      .then((r) => {
        if (r._tag === "Right") {
          download(r.right, format, k.redact);
        } else {
          const e = r.left;
          const why =
            e._tag === "TransportError" ? e.message : wireErrorMessage(e);
          toast.error(`Export failed: ${why}`);
        }
      })
      .finally(() => setBusy(null));
  };
  return (
    <fieldset
      aria-busy={busy !== null}
      className="m-0 flex min-w-0 items-center gap-1 border-0 p-0"
      data-testid="session-export"
    >
      <legend className="sr-only">Export session</legend>
      <DownloadIcon
        aria-hidden="true"
        className="size-3.5 text-muted-foreground"
      />
      {EXPORT_FORMATS.map((format) => (
        <Button
          aria-label={`Export as ${format}`}
          disabled={busy !== null}
          key={format}
          onClick={() => run(format)}
          size="sm"
          variant="ghost"
        >
          {format}
        </Button>
      ))}
    </fieldset>
  );
};
