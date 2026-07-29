import type { HighlightToken } from "@tanstack/highlight/core";
import { createHighlighter } from "@tanstack/highlight/core";
import { json } from "@tanstack/highlight/languages/json";
import { markdown } from "@tanstack/highlight/languages/markdown";
import { plaintext } from "@tanstack/highlight/languages/plaintext";
import { shell } from "@tanstack/highlight/languages/shell";
import { ts } from "@tanstack/highlight/languages/ts";

/** Languages registered in the shared highlighter, narrowed to what we render. */
const codeBlockLanguages = [
  "typescript",
  "bash",
  "json",
  "markdown",
  "text",
] as const;

export type CodeBlockLanguage = (typeof codeBlockLanguages)[number];

export interface CodeToken {
  className?: HighlightToken["className"];
  key: string;
  value: string;
}

export interface CodeLine {
  key: string;
  tokens: CodeToken[];
}

const highlighter = createHighlighter({
  fallbackLanguage: "plaintext",
  languages: [ts, shell, json, markdown, plaintext],
});

// The shell tokenizer backtracks quadratically on input that is not a shell
// command (750 KB of JSON takes ~62 s), so it gets a far tighter ceiling than
// the linear ts/json/markdown/plaintext tokenizers, which handle 1 MB in under
// 70 ms.
const highlightLimits: Record<CodeBlockLanguage, number> = {
  bash: 20_000,
  json: 1_000_000,
  markdown: 1_000_000,
  text: 1_000_000,
  typescript: 1_000_000,
};

/** Whether code is small enough to tokenize without blocking the main thread. */
export const isHighlightable = (code: string, language: CodeBlockLanguage) =>
  code.length <= highlightLimits[language];

const lineKey = (index: number) => `line-${index}`;

/**
 * Splits code into one classless token per line, for content too large to
 * tokenize without blocking the main thread.
 */
export const plainLines = (code: string): CodeLine[] =>
  code.split("\n").map((line, index) => {
    const key = lineKey(index);
    return {
      key,
      tokens: line === "" ? [] : [{ key: `${key}-0`, value: line }],
    };
  });

/**
 * Tokenizes code and regroups the flat token stream into lines, since a single
 * token may span several newlines.
 */
export const tokenizeLines = (
  code: string,
  language: CodeBlockLanguage
): CodeLine[] => {
  const { tokens } = highlighter.tokenize(code, { lang: language });
  let line: CodeLine = { key: lineKey(0), tokens: [] };
  const lines: CodeLine[] = [line];

  for (const token of tokens) {
    const parts = token.value.split("\n");
    for (const [partIndex, part] of parts.entries()) {
      if (partIndex > 0) {
        line = { key: lineKey(lines.length), tokens: [] };
        lines.push(line);
      }
      if (part === "") {
        continue;
      }
      line.tokens.push({
        className: token.className,
        key: `${line.key}-${line.tokens.length}`,
        value: part,
      });
    }
  }

  return lines;
};
