/** Pure parsers for memory bodies + the MEMORY.md index.
 *
 * Ported from `memory-view/scripts/lib/parse.ts`, decoupled from `node:fs` so the
 * Effect service owns all IO. These functions take already-read text + stat.
 */
import { createHash } from "node:crypto";
import { basename } from "node:path";
import { parseFrontmatter } from "./frontmatter";
import {
  type Frontmatter,
  type IndexBudget,
  type IndexEntry,
  type LinkRef,
  MAX_INDEX_BYTES,
  MAX_INDEX_LINES,
  type MemoryEntry,
  type MemoryIndex,
} from "./types";

const ANCHOR_RE = /#.*$/;
const ALIAS_RE = /\|.*$/;
const MD_EXT_RE = /\.md$/i;
const WIKI_LINK_RE = /\[\[([^\]]+)\]\]/g;
const MD_LINK_RE = /\[[^\]]*\]\(([^)]+)\)/g;
const EXTERNAL_RE = /^[a-z][a-z0-9+.-]*:\/\//i;
const WS_RE = /\s+/g;
const NEWLINE_RE = /\r?\n/;
const BARE_BULLET_RE = /^\s*[-*]\s+\S/;
const HEADING_RE = /(^|\n)#{1,6}\s/;
const INDEX_SEP = "(?:—|–|·|-{1,2})";
const INDEX_MD_RE = new RegExp(
  `^\\s*[-*]\\s+\\[([^\\]]+)\\]\\(([^)]+)\\)\\s*(?:${INDEX_SEP}\\s*(.*))?$`
);
const INDEX_WIKI_RE = new RegExp(
  `^\\s*[-*]\\s+\\[\\[([^\\]]+)\\]\\]\\s*(?:${INDEX_SEP}\\s*(.*))?$`
);

/** Normalize a link/target string to a comparable slug stem. */
export const normalizeSlug = (target: string): string => {
  const stem = target
    .trim()
    .replace(ANCHOR_RE, "")
    .replace(ALIAS_RE, "")
    .replace(MD_EXT_RE, "")
    .split("/")
    .pop();
  return (stem ?? "").trim().toLowerCase();
};

/** Extract `[[wiki]]` and `[label](target.md)` links from a body, with line numbers. */
const extractLinks = (body: string): LinkRef[] => {
  const links: LinkRef[] = [];
  body.split("\n").forEach((line, i) => {
    const lineNo = i + 1;
    for (const m of line.matchAll(WIKI_LINK_RE)) {
      const target = m[1];
      if (target === undefined) {
        continue;
      }
      links.push({
        kind: "wiki",
        rawTarget: target,
        targetSlug: normalizeSlug(target),
        line: lineNo,
      });
    }
    for (const m of line.matchAll(MD_LINK_RE)) {
      if (m[1] === undefined) {
        continue;
      }
      const href = m[1].trim();
      if (EXTERNAL_RE.test(href) || href.startsWith("#")) {
        continue;
      }
      links.push({
        kind: "markdown",
        rawTarget: href,
        targetSlug: normalizeSlug(href),
        line: lineNo,
      });
    }
  });
  return links;
};

/** Hash of the normalized body (lowercased, whitespace-collapsed) for dup detection. */
const hashBody = (body: string): string =>
  createHash("sha1")
    .update(body.toLowerCase().replace(WS_RE, " ").trim())
    .digest("hex");

/** Collect the optional convenience fields without ever emitting `undefined`. */
const optFields = (
  fm: Frontmatter
): Partial<Pick<MemoryEntry, "name" | "description" | "type">> => ({
  ...(fm.name === undefined ? {} : { name: fm.name }),
  ...(fm.description === undefined ? {} : { description: fm.description }),
  ...(fm.type === undefined ? {} : { type: fm.type }),
});

/**
 * Build a fully parsed `MemoryEntry` from already-read text + stat. `inIndex` is
 * supplied by the caller after the index has been parsed.
 */
export const buildEntry = (args: {
  readonly path: string;
  readonly text: string;
  readonly mtimeMs: number;
  readonly inIndex: boolean;
}): MemoryEntry => {
  const { path, text, mtimeMs, inIndex } = args;
  const { frontmatter, body } = parseFrontmatter(text);
  const fileName = basename(path);
  const allLines = text.split(NEWLINE_RE);
  return {
    slug: fileName.replace(MD_EXT_RE, ""),
    fileName,
    ...optFields(frontmatter),
    body,
    size: Buffer.byteLength(text, "utf8"),
    lines: allLines.length,
    mtime: mtimeMs > 0 ? new Date(mtimeMs).toISOString() : "",
    mtimeMs,
    bodyHash: hashBody(body),
    inIndex,
    links: extractLinks(body),
    frontmatter,
  };
};

/** Parse a single MEMORY.md line into an IndexEntry, or null for prose/headings. */
const parseIndexLine = (raw: string, lineNumber: number): IndexEntry | null => {
  if (raw.trim() === "") {
    return null;
  }
  const md = raw.match(INDEX_MD_RE);
  if (md?.[1] !== undefined && md[2] !== undefined) {
    const hook = md[3]?.trim();
    return {
      raw,
      lineNumber,
      label: md[1].trim(),
      target: md[2].trim(),
      targetSlug: normalizeSlug(md[2]),
      ...(hook ? { hook } : {}),
      malformed: false,
      kind: "markdown",
    };
  }
  const wiki = raw.match(INDEX_WIKI_RE);
  if (wiki?.[1] !== undefined) {
    const inner = wiki[1].trim();
    const hook = wiki[2]?.trim();
    return {
      raw,
      lineNumber,
      label: inner,
      target: inner,
      targetSlug: normalizeSlug(inner),
      ...(hook ? { hook } : {}),
      malformed: false,
      kind: "wiki",
    };
  }
  if (BARE_BULLET_RE.test(raw)) {
    return { raw, lineNumber, malformed: true, kind: "bare" };
  }
  return null;
};

/**
 * Parse MEMORY.md content into a `MemoryIndex`, detecting the monolithic-prose
 * case and which entries fall past the 200-line / 25 KB cliff (invisible to Claude).
 */
export const parseIndexContent = (args: {
  readonly raw: string;
  readonly path: string;
}): MemoryIndex => {
  const { raw, path } = args;
  const bytes = Buffer.byteLength(raw, "utf8");
  const rawLines = raw.split(NEWLINE_RE);

  const entries: IndexEntry[] = [];
  rawLines.forEach((line, i) => {
    const e = parseIndexLine(line, i + 1);
    if (e) {
      entries.push(e);
    }
  });

  const linkEntries = entries.filter((e) => e.targetSlug);
  const hasHeadings = HEADING_RE.test(raw);
  const isMonolithic =
    raw.trim().length > 0 && linkEntries.length === 0 && hasHeadings;

  const lineBytes = rawLines.map((l) => Buffer.byteLength(`${l}\n`, "utf8"));
  const belowFoldEntries = entries.filter((e) => {
    const byteOffset = lineBytes
      .slice(0, e.lineNumber - 1)
      .reduce((s, b) => s + b, 0);
    return e.lineNumber > MAX_INDEX_LINES || byteOffset >= MAX_INDEX_BYTES;
  });

  return {
    exists: true,
    path,
    raw,
    bytes,
    lines: rawLines.length,
    entries,
    isMonolithic,
    overBudget: rawLines.length > MAX_INDEX_LINES || bytes > MAX_INDEX_BYTES,
    belowFoldEntries,
  };
};

/** Derive the headline budget gauge from a parsed index (or its absence). */
export const indexBudget = (index: MemoryIndex | null): IndexBudget => {
  if (!index) {
    return {
      lines: 0,
      maxLines: MAX_INDEX_LINES,
      bytes: 0,
      maxBytes: MAX_INDEX_BYTES,
      overBudget: false,
      belowFoldCount: 0,
      kind: "absent",
    };
  }
  return {
    lines: index.lines,
    maxLines: MAX_INDEX_LINES,
    bytes: index.bytes,
    maxBytes: MAX_INDEX_BYTES,
    overBudget: index.overBudget,
    belowFoldCount: index.belowFoldEntries.length,
    kind: index.isMonolithic ? "monolithic" : "index",
  };
};
