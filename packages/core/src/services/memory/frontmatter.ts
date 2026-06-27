/** Tolerant, zero-dependency YAML frontmatter parse + serialize for memory files.
 *
 * Ported from `memory-view/scripts/lib/frontmatter.ts`. Handles the three on-disk
 * shapes (nested `metadata:` block, legacy flat, none) without a YAML library and
 * preserves the raw block so unchanged frontmatter re-emits byte-for-byte.
 */
import type { Frontmatter, SchemaShape } from "./types";

const BOM_RE = /^﻿/;
const NEWLINE_RE = /\r?\n/;
const INDENTED_RE = /^\s+\S/;
const METADATA_OPEN_RE = /^metadata\s*:\s*$/;
const METADATA_WS_RE = /^metadata:[ \t]+$/;

/** Writable working copy of `Frontmatter` for the parser + edit paths. */
export interface WritableFrontmatter {
  created?: string | undefined;
  description?: string | undefined;
  descriptionQuoted: boolean;
  extra: Record<string, string>;
  hadTrailingMetadataWs: boolean;
  name?: string | undefined;
  nodeType?: string | undefined;
  originSessionId?: string | undefined;
  raw: string;
  shape: SchemaShape;
  type?: string | undefined;
  updated?: string | undefined;
}

/** A fresh frontmatter accumulator (shape "none"). */
export const emptyFrontmatter = (): WritableFrontmatter => ({
  raw: "",
  shape: "none",
  extra: {},
  hadTrailingMetadataWs: false,
  descriptionQuoted: false,
});

/** Strip a single matched pair of surrounding quotes; report whether it was quoted. */
const unquote = (v: string): { value: string; quoted: boolean } => {
  const t = v.trim();
  const first = t[0];
  const last = t.at(-1);
  if (
    t.length >= 2 &&
    ((first === '"' && last === '"') || (first === "'" && last === "'"))
  ) {
    return { value: t.slice(1, -1), quoted: true };
  }
  return { value: t, quoted: false };
};

/** Split a `key: value` line on the first colon. Returns null when no colon. */
const splitKv = (line: string): { key: string; value: string } | null => {
  const idx = line.indexOf(":");
  if (idx < 0) {
    return null;
  }
  return { key: line.slice(0, idx).trim(), value: line.slice(idx + 1) };
};

/** Assign one nested-block key onto the frontmatter accumulator. */
const assignNested = (
  fm: WritableFrontmatter,
  key: string,
  value: string
): void => {
  if (key === "node_type") {
    fm.nodeType = value;
  } else if (key === "type") {
    fm.type = value;
  } else if (key === "originSessionId") {
    fm.originSessionId = value;
  } else if (key === "created") {
    fm.created = value;
  } else if (key === "updated") {
    fm.updated = value;
  } else {
    fm.extra[key] = value;
  }
};

/** Assign one top-level key onto the frontmatter accumulator. */
const assignTop = (
  fm: WritableFrontmatter,
  key: string,
  value: string,
  quoted: boolean
): void => {
  switch (key) {
    case "name":
      fm.name = value;
      break;
    case "description":
      fm.description = value;
      fm.descriptionQuoted = quoted;
      break;
    case "type":
      fm.type = value;
      break;
    case "node_type":
      fm.nodeType = value;
      break;
    case "originSessionId":
      fm.originSessionId = value;
      break;
    case "created":
      fm.created = value;
      break;
    case "updated":
      fm.updated = value;
      break;
    default:
      fm.extra[key] = value;
  }
};

/**
 * Parse a file's full text into its frontmatter and body. With no leading `---`
 * fence the whole text is the body and shape is "none".
 */
export const parseFrontmatter = (
  fileText: string
): { frontmatter: Frontmatter; body: string } => {
  const text = fileText.replace(BOM_RE, "");
  const lines = text.split(NEWLINE_RE);

  if (lines[0]?.trim() !== "---") {
    return { frontmatter: emptyFrontmatter(), body: text };
  }

  let close = -1;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i]?.trim() === "---") {
      close = i;
      break;
    }
  }
  if (close < 0) {
    return { frontmatter: emptyFrontmatter(), body: text };
  }

  const blockLines = lines.slice(1, close);
  const fm: WritableFrontmatter = {
    raw: blockLines.join("\n"),
    shape: "flat",
    extra: {},
    hadTrailingMetadataWs: false,
    descriptionQuoted: false,
  };
  const body = lines.slice(close + 1).join("\n");

  let inMetadata = false;
  for (const line of blockLines) {
    if (line.trim() === "") {
      continue;
    }
    const indented = INDENTED_RE.test(line);

    if (METADATA_OPEN_RE.test(line)) {
      fm.shape = "nested";
      inMetadata = true;
      fm.hadTrailingMetadataWs = METADATA_WS_RE.test(line);
      continue;
    }

    if (inMetadata && indented) {
      const kv = splitKv(line);
      if (kv) {
        assignNested(fm, kv.key, unquote(kv.value).value);
      }
      continue;
    }

    inMetadata = false;
    const kv = splitKv(line);
    if (!kv) {
      continue;
    }
    const { value, quoted } = unquote(kv.value);
    assignTop(fm, kv.key, value, quoted);
  }

  return { frontmatter: fm, body };
};

/** Emit a `key: value` line for a defined value, or nothing. */
const kvLine = (key: string, value: string | undefined): string[] =>
  value === undefined ? [] : [`${key}: ${value}`];

/** Header lines (name + description), honoring the on-disk quoting. */
const headerLines = (fm: Frontmatter): string[] => {
  const out = kvLine("name", fm.name);
  if (fm.description !== undefined) {
    out.push(
      `description: ${fm.descriptionQuoted ? JSON.stringify(fm.description) : fm.description}`
    );
  }
  return out;
};

/** The shared metadata fields, indented when nested. */
const metaLines = (fm: Frontmatter, indent: string): string[] => [
  ...(indent ? kvLine(`${indent}node_type`, fm.nodeType) : []),
  ...kvLine(`${indent}type`, fm.type),
  ...kvLine(`${indent}originSessionId`, fm.originSessionId),
  ...kvLine(`${indent}created`, fm.created),
  ...kvLine(`${indent}updated`, fm.updated),
];

/**
 * Re-emit a frontmatter block (no fences) canonically in the file's own shape.
 * Used only when frontmatter fields change; unchanged frontmatter keeps `raw`.
 */
export const serializeFrontmatterBlock = (fm: Frontmatter): string => {
  const body =
    fm.shape === "nested"
      ? ["metadata:", ...metaLines(fm, "  ")]
      : metaLines(fm, "");
  const extra = Object.entries(fm.extra).map(([k, v]) => `${k}: ${v}`);
  return [...headerLines(fm), ...body, ...extra].join("\n");
};

/**
 * Recompose a file's full text from frontmatter + body. Lossless for unchanged
 * frontmatter: `parseFrontmatter(composeFile(parseFrontmatter(t))) === t`.
 */
export const composeFile = (args: {
  readonly frontmatter: Frontmatter;
  readonly body: string;
}): string =>
  args.frontmatter.shape === "none"
    ? args.body
    : `---\n${args.frontmatter.raw}\n---\n${args.body}`;
