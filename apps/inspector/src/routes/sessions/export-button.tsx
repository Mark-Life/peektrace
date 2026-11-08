/** Download the loaded session as HTML, Markdown or JSON. Renders the data
 * already on screen, so the file matches the redaction toggle. */
import {
  EXPORT_FORMATS,
  type ExportFormat,
} from "@workspace/core/services/sessions/export/index";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Button } from "@workspace/ui/components/button";
import { DownloadIcon } from "lucide-react";
import { exportFile } from "../../lib/export-file";

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

export const ExportButtons = ({
  a,
  redacted,
}: {
  readonly a: AnalyzedSession;
  readonly redacted: boolean;
}) => (
  <fieldset
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
        key={format}
        onClick={() => download(a, format, redacted)}
        size="sm"
        variant="ghost"
      >
        {format}
      </Button>
    ))}
  </fieldset>
);
