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

const heading = (e: ExportEvent, position: number) => {
  const tail = tailOf(e);
  return `### ${position}. ${oneLine(whoOf(e), LABEL_MAX)}${tail ? ` (${tail})` : ""}`;
};

const FENCE_LINE = /^ {0,3}(`{3,}|~{3,})(.*)$/;

const COMMENT_OPEN = "<!--";
const COMMENT_LINE = /^ {0,3}<!--/;

/** The body with any code fence or HTML comment it leaves open closed, so
 * later headings stay outside it. */
const closeOpenFence = (body: string) => {
  let open: string | undefined;
  for (const line of body.split("\n")) {
    const m = FENCE_LINE.exec(line);
    if (!m) {
      continue;
    }
    const [, run = "", info = ""] = m;
    if (open === undefined) {
      // A backtick run with a backtick in its info string is an inline span.
      if (!(run.startsWith("`") && info.includes("`"))) {
        open = run;
      }
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

/** Close the last line-start `<!--` that has no `-->` after it. Skipped when
 * the body ends inside a code fence, where comments are plain text. */
const closeOpenComment = (body: string) => {
  const lines = body.split("\n");
  const start = lines.map((l) => COMMENT_LINE.test(l)).lastIndexOf(true);
  if (start === -1) {
    return body;
  }
  const rest = lines.slice(start).join("\n");
  const afterOpen = rest.slice(
    rest.indexOf(COMMENT_OPEN) + COMMENT_OPEN.length
  );
  return afterOpen.includes("-->") ? body : `${body}\n-->`;
};

const closeOpenBlocks = (body: string) => {
  const fenced = closeOpenFence(body);
  return fenced === body ? closeOpenComment(body) : fenced;
};

const eventBlock = (e: ExportEvent, i: number) => {
  const parts = [heading(e, i + 1)];
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
    parts.push(closeOpenBlocks(e.body));
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
