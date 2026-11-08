import {
  type ExportFormat,
  renderExport,
} from "@workspace/core/services/sessions/export/index";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";

const MIME: Record<ExportFormat, string> = {
  html: "text/html",
  md: "text/markdown",
  json: "application/json",
};

const SHORT_ID_LENGTH = 8;

/** File name, MIME type and text of a session export. `redacted` is passed
 * through to the renderer. */
export const exportFile = (
  a: AnalyzedSession,
  format: ExportFormat,
  redacted: boolean,
  exportedAt = new Date().toISOString()
) => ({
  name: `session-${a.sessionId.slice(0, SHORT_ID_LENGTH)}.${format}`,
  type: `${MIME[format]};charset=utf-8`,
  text: renderExport(format, a, { exportedAt, redacted }),
});
