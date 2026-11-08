import type { ParsedSession } from "../schema";
import {
  type ExportDoc,
  type ExportEvent,
  type ExportOptions,
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

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

/** Escape text for HTML element content and quoted attribute values. */
const escapeHtml = (text: string) =>
  text.replace(/[&<>"']/g, (c) => ESCAPES[c] ?? c);

const CSS = `
:root{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--mute:#666;--line:#ddd;--card:#f6f6f6;--user:#e8f0fe;--err:#c0392b}
@media (prefers-color-scheme:dark){:root{--bg:#141414;--fg:#e8e8e8;--mute:#999;--line:#333;--card:#1e1e1e;--user:#1c2a44;--err:#ff8a80}}
body{margin:0 auto;max-width:56rem;padding:1.5rem;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}
h1{font-size:1.4rem;margin:0 0 .5rem}
dl{display:grid;grid-template-columns:max-content 1fr;gap:.15rem 1rem;color:var(--mute);margin:0 0 1.5rem}
dt{font-weight:600}dd{margin:0;overflow-wrap:anywhere}
.msg,details{border:1px solid var(--line);border-radius:6px;margin:.75rem 0;padding:.6rem .8rem;background:var(--card)}
.msg.user{background:var(--user)}
.who{font-weight:600;font-size:.85rem;color:var(--mute);margin:0 0 .3rem}
summary{cursor:pointer;color:var(--mute);font-size:.85rem}
pre{white-space:pre-wrap;overflow-wrap:anywhere;margin:.4rem 0 0;font:13px/1.45 ui-monospace,monospace}
.note{color:var(--mute);font-style:italic;font-size:.85rem}
.err summary{color:var(--err)}
`;

const meta = (d: ExportDoc) =>
  metaRows(d)
    .map(([k, v]) => `<dt>${escapeHtml(k)}</dt><dd>${escapeHtml(v)}</dd>`)
    .join("");

const subagents = (d: ExportDoc) =>
  d.subagents.length === 0
    ? ""
    : `<h2>Subagents</h2><ul>${d.subagents
        .map(
          (s) =>
            `<li><code>${escapeHtml(s.id)}</code>: ${escapeHtml(subagentSummary(s))}</li>`
        )
        .join("")}</ul>`;

const eventHtml = (e: ExportEvent) => {
  const who = escapeHtml(oneLine(whoOf(e), LABEL_MAX));
  const tail = tailOf(e);
  const head = `${who}${tail ? ` (${escapeHtml(tail)})` : ""}`;
  const note =
    e.note === undefined
      ? ""
      : `<p class="note">${escapeHtml(oneLine(e.note, NOTE_MAX))}</p>`;
  const body = `<pre>${escapeHtml(e.body)}</pre>`;
  if (e.role !== "other") {
    return `<section class="msg ${e.role}"><h3 class="who">${head}</h3>${note}${body}</section>`;
  }
  const title =
    e.title !== "" && e.title !== e.label
      ? ` — ${escapeHtml(oneLine(e.title))}`
      : "";
  return `<details${e.isError ? ' class="err"' : ""}><summary>${head}${title}</summary>${note}${body}</details>`;
};

/** Render a session as one self-contained HTML string: inline CSS, no script,
 * no external loads, every text value escaped. */
export const renderHtml = (session: ParsedSession, opts: ExportOptions) => {
  const d = prepareExport(session, opts);
  const title = escapeHtml(oneLine(d.title, TITLE_MAX));
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'">
<meta name="referrer" content="no-referrer">
<title>${title}</title>
<style>${CSS}</style>
</head>
<body>
<h1>${title}</h1>
<dl>${meta(d)}</dl>
${subagents(d)}
<h2>Timeline</h2>
${d.events.map(eventHtml).join("\n")}
</body>
</html>
`;
};
