/** Session export: the server renders the file (`GET /api/sessions/{id}/export`),
 * with the same redaction as the screen. */
import type { EXPORT_FORMATS } from "@workspace/view-core/constants.gen";
import { apiPath } from "./api";
import type { SessionKey } from "./session-data";

export type ExportFormat = (typeof EXPORT_FORMATS)[number];

const SHORT_ID_LENGTH = 8;

/** Download name of an export: the short id and the format. */
export const exportName = (id: string, format: ExportFormat) =>
  `session-${id.slice(0, SHORT_ID_LENGTH)}.${format}`;

/** Route and query of a session's export in `format`. */
export const exportRequest = (k: SessionKey, format: ExportFormat) => ({
  path: apiPath("sessions", k.id, "export"),
  query: {
    format,
    parent: k.parent ?? undefined,
    redact: k.redact ? undefined : "false",
  },
});
