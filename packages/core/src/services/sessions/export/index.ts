/** Session export: render a parsed session as HTML, Markdown or JSON.
 *
 * Pure functions over `ParsedSession`; callers redact first (the `sessions.get`
 * RPC does it by default) and pass `redacted` so home paths get collapsed too.
 */
import type { ParsedSession } from "../schema";
import { renderHtml } from "./html";
import { renderMarkdown } from "./markdown";
import { type ExportFormat, type ExportOptions, prepareExport } from "./shared";

export { escapeHtml, renderHtml } from "./html";
export { renderMarkdown } from "./markdown";
export {
  collapseHomePaths,
  EXPORT_FORMATS,
  type ExportFormat,
  type ExportOptions,
  fenceFor,
} from "./shared";

const EXPORT_SCHEMA = "peektrace.session-export";
const EXPORT_VERSION = 1;

/** Render a session as JSON built from an explicit field whitelist. */
export const renderJson = (session: ParsedSession, opts: ExportOptions) => {
  const d = prepareExport(session, opts);
  return `${JSON.stringify(
    {
      schema: EXPORT_SCHEMA,
      version: EXPORT_VERSION,
      redacted: d.redacted,
      exportedAt: d.exportedAt,
      session: {
        sessionId: d.sessionId,
        provider: d.provider,
        models: d.models,
        title: d.title,
        gitBranch: d.gitBranch,
        cwd: d.cwd,
        startedAt: d.startedAt,
        endedAt: d.endedAt,
        events: d.events.map((e) => ({
          index: e.index,
          kind: e.kind,
          ts: e.ts,
          label: e.label,
          title: e.title,
          body: e.body,
          toolName: e.toolName,
          toolUseId: e.toolUseId,
          isError: e.isError,
          isMeta: e.isMeta,
          isSidechain: e.isSidechain,
          promptOrigin: e.promptOrigin,
          tokensEst: e.tokensEst,
          tokensMeasured: e.tokensMeasured,
          contextEdit: e.contextEdit,
        })),
        subagents: d.subagents,
      },
    },
    null,
    2
  )}\n`;
};

export const renderExport = (
  format: ExportFormat,
  session: ParsedSession,
  opts: ExportOptions
) => {
  switch (format) {
    case "html":
      return renderHtml(session, opts);
    case "md":
      return renderMarkdown(session, opts);
    case "json":
      return renderJson(session, opts);
    default:
      return format satisfies never;
  }
};
