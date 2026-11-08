import type { ParsedSession } from "../schema";
import {
  type ExportEvent,
  type ExportOptions,
  fenceFor,
  LABEL_MAX,
  metaRows,
  NOTE_MAX,
  oneLine,
  prepareExport,
  subagentSummary,
  TITLE_MAX,
  tailOf,
  whoOf,
} from "./shared";

const heading = (e: ExportEvent) => {
  const tail = tailOf(e);
  return `### ${e.index + 1}. ${oneLine(whoOf(e), LABEL_MAX)}${tail ? ` (${tail})` : ""}`;
};

const FENCE_LINE = /^ {0,3}(`{3,}|~{3,})(.*)$/;

/** The body with any code fence it leaves open closed, so later headings
 * stay outside the code block. */
const closeOpenFence = (body: string) => {
  let open: string | undefined;
  for (const line of body.split("\n")) {
    const m = FENCE_LINE.exec(line);
    if (!m) {
      continue;
    }
    const [, run = "", info = ""] = m;
    if (open === undefined) {
      open = run;
    } else if (
      run.startsWith(open[0] ?? "") &&
      run.length >= open.length &&
      info.trim() === ""
    ) {
      open = undefined;
    }
  }
  return open === undefined ? body : `${body}\n${open}`;
};

const eventBlock = (e: ExportEvent) => {
  const parts = [heading(e)];
  if (e.role === "other" && e.title !== "" && e.title !== e.label) {
    parts.push(`*${oneLine(e.title)}*`);
  }
  if (e.note !== undefined) {
    parts.push(`> ${oneLine(e.note, NOTE_MAX)}`);
  }
  if (e.role === "other") {
    const fence = fenceFor(e.body);
    parts.push(`${fence}\n${e.body}\n${fence}`);
  } else {
    parts.push(closeOpenFence(e.body));
  }
  return parts.join("\n\n");
};

/** Render a session as Markdown. Chat messages stay raw markdown; tool calls,
 * results, thinking and other events sit in fenced blocks. */
export const renderMarkdown = (session: ParsedSession, opts: ExportOptions) => {
  const d = prepareExport(session, opts);
  const subs = d.subagents.map((s) => `- \`${s.id}\`: ${subagentSummary(s)}`);
  return `${[
    `# ${oneLine(d.title, TITLE_MAX)}`,
    metaRows(d)
      .map(([k, v]) => `- ${k}: ${v}`)
      .join("\n"),
    ...(subs.length > 0 ? ["## Subagents", subs.join("\n")] : []),
    "## Timeline",
    ...d.events.map(eventBlock),
  ].join("\n\n")}\n`;
};
