/** Syntax-highlighted body rendering for the session history.
 *
 * Tokenizes with the shared `@workspace/core/highlight` tokenizer — the same one
 * the web inspector uses — and paints the whole body as one `StyledText` of
 * coloured chunks, using a GitHub-dark palette so terminal and web match.
 * Tool-call inputs render as TypeScript/bash, tool results and assistant text as
 * markdown, structured payloads as JSON.
 */
import type { RGBA, TextChunk } from "@opentui/core";
import {
  createTextAttributes,
  RGBA as RGBAColor,
  StyledText,
} from "@opentui/core";
import type {
  CodeBlockLanguage,
  CodeLine,
  HighlightTokenClass,
} from "@workspace/core/highlight";
import {
  isHighlightable,
  plainLines,
  tokenizeLines,
} from "@workspace/core/highlight";
import { useMemo } from "react";
import { C, sanitize } from "./theme";

const BOLD = createTextAttributes({ bold: true });
const ITALIC = createTextAttributes({ italic: true });
const UNDERLINE = createTextAttributes({ underline: true });
const PLAIN = createTextAttributes({});

interface TokenStyle {
  readonly attributes: number;
  readonly fg: RGBA;
}

const colorCache = new Map<string, RGBA>();

/** Parse a hex colour once per distinct value, so equal colours share identity. */
const rgba = (hex: string): RGBA => {
  const cached = colorCache.get(hex);
  if (cached !== undefined) {
    return cached;
  }
  const parsed = RGBAColor.fromHex(hex);
  colorCache.set(hex, parsed);
  return parsed;
};

/** Style for classless runs and for any class the highlighter adds later. */
const DEFAULT_STYLE: TokenStyle = { attributes: PLAIN, fg: rgba(C.text) };

/** GitHub-dark style per highlighter token class (exhaustive by construction). */
const TOKEN_STYLE: Record<HighlightTokenClass, TokenStyle> = {
  attr: { attributes: PLAIN, fg: rgba("#79c0ff") },
  "code-inline": { attributes: PLAIN, fg: rgba("#a5d6ff") },
  command: { attributes: PLAIN, fg: rgba("#d2a8ff") },
  comment: { attributes: ITALIC, fg: rgba("#8b949e") },
  deleted: { attributes: PLAIN, fg: rgba("#ffa198") },
  function: { attributes: PLAIN, fg: rgba("#d2a8ff") },
  heading: { attributes: BOLD, fg: rgba("#58a6ff") },
  inserted: { attributes: PLAIN, fg: rgba("#7ee787") },
  keyword: { attributes: BOLD, fg: rgba("#ff7b72") },
  link: { attributes: UNDERLINE, fg: rgba("#58a6ff") },
  literal: { attributes: PLAIN, fg: rgba("#79c0ff") },
  meta: { attributes: PLAIN, fg: rgba("#8b949e") },
  number: { attributes: PLAIN, fg: rgba("#79c0ff") },
  operator: { attributes: PLAIN, fg: rgba("#ff7b72") },
  property: { attributes: PLAIN, fg: rgba("#79c0ff") },
  selector: { attributes: PLAIN, fg: rgba("#7ee787") },
  string: { attributes: PLAIN, fg: rgba("#a5d6ff") },
  tag: { attributes: PLAIN, fg: rgba("#7ee787") },
  type: { attributes: PLAIN, fg: rgba("#ffa657") },
  variable: { attributes: PLAIN, fg: rgba("#e6edf3") },
};

/**
 * Style for a token class. Classless runs and any class not in the palette (a
 * newer `@tanstack/highlight` can emit one) fall back to body text rather than
 * throwing inside render.
 */
const styleOf = (className: string | undefined): TokenStyle => {
  if (className === undefined) {
    return DEFAULT_STYLE;
  }
  return TOKEN_STYLE[className as HighlightTokenClass] ?? DEFAULT_STYLE;
};

/** Languages we tokenize; `plain` bypasses the highlighter entirely. */
export type Lang = Exclude<CodeBlockLanguage, "text"> | "plain";

/** Longest single line kept before eliding — a base64 blob is one huge line. */
const MAX_LINE_CHARS = 1000;
/** Total characters kept per body; dense minified payloads hit this first. */
const MAX_BODY_CHARS = 20_000;

/** Trim one line to `MAX_LINE_CHARS`, appending an elision marker. */
const capLine = (line: string): string =>
  line.length <= MAX_LINE_CHARS
    ? line
    : `${line.slice(0, MAX_LINE_CHARS)}… +${line.length - MAX_LINE_CHARS} more chars`;

/**
 * Bound a body three ways before it reaches the tokenizer: `maxLines` lines, at
 * most `MAX_LINE_CHARS` per line, and `MAX_BODY_CHARS` overall. Every bound is
 * needed — one huge base64 line passes the line count, and minified JSON passes
 * both counts while still producing tens of thousands of coloured runs.
 */
const capLines = (text: string, maxLines: number): string => {
  const lines = text.split("\n");
  const kept: string[] = [];
  let budget = MAX_BODY_CHARS;
  for (const line of lines) {
    if (kept.length >= maxLines || budget <= 0) {
      break;
    }
    const capped = capLine(line);
    kept.push(capped);
    budget -= capped.length + 1;
  }
  if (kept.length < lines.length) {
    kept.push(`… +${lines.length - kept.length} more lines`);
  }
  return kept.join("\n");
};

/**
 * Flatten tokenized lines into one styled run, merging neighbours that share a
 * colour and attributes. One chunk per style change (rather than one renderable
 * per token) is what keeps expanding a whole transcript off the render loop.
 */
const styledOf = (lines: CodeLine[]): StyledText => {
  const chunks: TextChunk[] = [];
  let open: TextChunk | undefined;
  const push = (text: string, style: TokenStyle) => {
    if (
      open !== undefined &&
      open.fg === style.fg &&
      open.attributes === style.attributes
    ) {
      open.text += text;
      return;
    }
    open = {
      __isChunk: true,
      attributes: style.attributes,
      fg: style.fg,
      text,
    };
    chunks.push(open);
  };

  for (const [index, line] of lines.entries()) {
    if (index > 0) {
      push("\n", DEFAULT_STYLE);
    }
    for (const token of line.tokens) {
      push(token.value, styleOf(token.className));
    }
  }
  return new StyledText(chunks);
};

/**
 * Tokenized code as one word-wrapping `<text>`. Content past the tokenizer's
 * per-language size cap falls back to unstyled lines rather than blocking the
 * terminal.
 */
const TokenLines = ({
  code,
  language,
}: {
  readonly code: string;
  readonly language: CodeBlockLanguage;
}) => {
  const styled = useMemo(
    () =>
      styledOf(
        isHighlightable(code, language)
          ? tokenizeLines(code, language)
          : plainLines(code)
      ),
    [code, language]
  );
  return <text content={styled} fg={C.text} wrapMode="word" />;
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
  return <TokenLines code={clean} language={lang} />;
};
