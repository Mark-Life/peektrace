/** Download the session as HTML, Markdown or JSON. The server renders the
 * file with the same redaction as the screen. */
import { Button } from "@workspace/ui/components/button";
import { EXPORT_FORMATS } from "@workspace/view-core/constants.gen";
import { Effect } from "effect";
import { DownloadIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { apiBlob } from "../../lib/api";
import { exportName, exportRequest } from "../../lib/export-file";
import type { ExportFormat } from "../../lib/export-file";
import type { SessionKey } from "../../lib/session-data";
import { wireErrorMessage } from "../../lib/wire-error";

const download = (blob: Blob, name: string) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
};

const exportOf = (k: SessionKey, format: ExportFormat) => {
  const { path, query } = exportRequest(k, format);

  return apiBlob(path, query);
};

export const ExportButtons = ({ k }: { readonly k: SessionKey }) => {
  const [busy, setBusy] = useState<ExportFormat | null>(null);

  const run = (format: ExportFormat) => {
    setBusy(format);
    Effect.runPromise(Effect.either(exportOf(k, format)))
      .then((r) => {
        if (r._tag === "Right") {
          download(r.right, exportName(k.id, format));
        } else {
          const e = r.left;

          const why =
            e._tag === "TransportError" ? e.message : wireErrorMessage(e);

          toast.error(`Export failed: ${why}`);
        }
      })
      .finally(() => {
        setBusy(null);
      });
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
        className="text-muted-foreground size-3.5"
      />
      {EXPORT_FORMATS.map((format) => (
        <Button
          aria-label={`Export as ${format}`}
          disabled={busy !== null}
          key={format}
          onClick={() => {
            run(format);
          }}
          size="sm"
          variant="ghost"
        >
          {format}
        </Button>
      ))}
    </fieldset>
  );
};
