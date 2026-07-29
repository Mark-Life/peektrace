/** Syntax-highlighted body rendering for the session history.
 *
 * Wraps OpenTUI's Tree-sitter `<code>` element with a GitHub-dark palette that
 * matches the web inspector, and a `<Highlighted>` component that sanitizes the
 * content, caps it to a line budget, and falls back to plain `<text>` for
 * unsupported languages. Tool-call inputs render as TypeScript/bash, tool
 * results and assistant text as markdown, structured payloads as JSON.
 */
import { RGBA, SyntaxStyle } from "@opentui/core";
import { C, sanitize } from "./theme";

const hex = (value: string) => RGBA.fromHex(value);

/** GitHub-dark token theme shared by every highlighted body. */
export const CODE_STYLE = SyntaxStyle.fromStyles({
  keyword: { fg: hex("#ff7b72"), bold: true },
  "keyword.import": { fg: hex("#ff7b72"), bold: true },
  "keyword.operator": { fg: hex("#ff7b72") },
  string: { fg: hex("#a5d6ff") },
  "string.special": { fg: hex("#a5d6ff") },
  comment: { fg: hex("#8b949e"), italic: true },
  number: { fg: hex("#79c0ff") },
  boolean: { fg: hex("#79c0ff") },
  constant: { fg: hex("#79c0ff") },
  "constant.builtin": { fg: hex("#79c0ff") },
  function: { fg: hex("#d2a8ff") },
  "function.call": { fg: hex("#d2a8ff") },
  "function.method.call": { fg: hex("#d2a8ff") },
  type: { fg: hex("#ffa657") },
  constructor: { fg: hex("#ffa657") },
  variable: { fg: hex("#e6edf3") },
  "variable.member": { fg: hex("#79c0ff") },
  property: { fg: hex("#79c0ff") },
  operator: { fg: hex("#ff7b72") },
  punctuation: { fg: hex("#c9d1d9") },
  "punctuation.bracket": { fg: hex("#c9d1d9") },
  "punctuation.delimiter": { fg: hex("#8b949e") },
  "markup.heading": { fg: hex("#58a6ff"), bold: true },
  "markup.bold": { fg: hex("#f0f6fc"), bold: true },
  "markup.strong": { fg: hex("#f0f6fc"), bold: true },
  "markup.italic": { fg: hex("#f0f6fc"), italic: true },
  "markup.list": { fg: hex("#ff7b72") },
  "markup.quote": { fg: hex("#8b949e"), italic: true },
  "markup.raw": { fg: hex("#a5d6ff") },
  "markup.raw.block": { fg: hex("#a5d6ff") },
  "markup.link": { fg: hex("#58a6ff"), underline: true },
  "markup.link.url": { fg: hex("#58a6ff"), underline: true },
  default: { fg: hex("#c9d1d9") },
});

/** Languages we hand to Tree-sitter (all verified to render); `plain` bypasses it. */
export type Lang = "typescript" | "bash" | "json" | "markdown" | "plain";

/** Keep a body to at most `maxLines` lines, appending an elision marker. */
const capLines = (text: string, maxLines: number): string => {
  const lines = text.split("\n");
  if (lines.length <= maxLines) {
    return text;
  }
  return `${lines.slice(0, maxLines).join("\n")}\n… +${lines.length - maxLines} more lines`;
};

/**
 * Render `content` with syntax highlighting for `lang`, sanitized and capped to
 * `maxLines`. `plain` (and empty content) render as dim word-wrapped text.
 */
export const Highlighted = ({
  content,
  lang,
  maxLines = 200,
}: {
  readonly content: string;
  readonly lang: Lang;
  readonly maxLines?: number;
}) => {
  const clean = capLines(sanitize(content), maxLines);
  if (lang === "plain" || clean.trim() === "") {
    return <text fg={C.text}>{clean === "" ? "—" : clean}</text>;
  }
  return (
    <code
      content={clean}
      filetype={lang}
      syntaxStyle={CODE_STYLE}
      wrapMode="word"
    />
  );
};
