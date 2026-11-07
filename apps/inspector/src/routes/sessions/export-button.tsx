/** Download the loaded session as HTML, Markdown or JSON. Renders the data
 * already on screen, so the file matches the redaction toggle. */
import {
  EXPORT_FORMATS,
  type ExportFormat,
  renderExport,
} from "@workspace/core/services/sessions/export/index";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Button } from "@workspace/ui/components/button";
import { DownloadIcon } from "lucide-react";

const MIME: Record<ExportFormat, string> = {
  html: "text/html",
  md: "text/markdown",
  json: "application/json",
};

const download = (
  a: AnalyzedSession,
  format: ExportFormat,
  redacted: boolean
) => {
  const text = renderExport(format, a, {
    exportedAt: new Date().toISOString(),
    redacted,
  });
  const url = URL.createObjectURL(
    new Blob([text], { type: `${MIME[format]};charset=utf-8` })
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = `session-${a.sessionId.slice(0, 8)}.${format}`;
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
  <div className="flex items-center gap-1" data-testid="session-export">
    <DownloadIcon className="size-3.5 text-muted-foreground" />
    {EXPORT_FORMATS.map((format) => (
      <Button
        key={format}
        onClick={() => download(a, format, redacted)}
        size="sm"
        variant="ghost"
      >
        {format}
      </Button>
    ))}
  </div>
);
