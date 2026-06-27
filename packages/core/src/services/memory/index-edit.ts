/** Pure string transforms on MEMORY.md content: build a canonical pointer line and
 * insert / update / remove the line for a given file slug. Mirrors the index line
 * shape from `memory-view/scripts/lib/reindex.ts` (`- [Label](file.md) — hook`).
 */
import { normalizeSlug } from "./parse";

const WS_RE = /\s+/g;
const TRAILING_NL_RE = /\n+$/;
const MD_POINTER_RE = /^\s*[-*]\s+\[[^\]]+\]\(([^)]+)\)/;
const WIKI_POINTER_RE = /^\s*[-*]\s+\[\[([^\]]+)\]\]/;

/** Collapse internal whitespace to single spaces and trim. */
const oneLine = (s: string): string => s.replace(WS_RE, " ").trim();

/** Canonical `- [Label](file.md) — hook` pointer line for one content file. */
export const indexLineFor = (args: {
  readonly label: string;
  readonly fileName: string;
  readonly hook?: string;
}): string => {
  const hook = oneLine(args.hook ?? "");
  const base = `- [${args.label.trim()}](${args.fileName})`;
  return hook ? `${base} — ${hook}` : base;
};

/** Append a pointer line to the index content (creating it when empty). */
export const insertIndexLine = (raw: string, line: string): string => {
  if (raw.trim() === "") {
    return `${line}\n`;
  }
  const trimmed = raw.replace(TRAILING_NL_RE, "");
  return `${trimmed}\n${line}\n`;
};

/** Does this raw MEMORY.md line point at the given file slug? */
const lineTargetsSlug = (raw: string, slug: string): boolean => {
  const md = raw.match(MD_POINTER_RE);
  if (md?.[1] !== undefined) {
    return normalizeSlug(md[1]) === slug;
  }
  const wiki = raw.match(WIKI_POINTER_RE);
  if (wiki?.[1] !== undefined) {
    return normalizeSlug(wiki[1]) === slug;
  }
  return false;
};

/** Remove every pointer line for `slug`; report whether anything was removed. */
export const removeIndexLine = (
  raw: string,
  slug: string
): { content: string; removed: boolean } => {
  const target = normalizeSlug(slug);
  const lines = raw.split("\n");
  const kept = lines.filter((l) => !lineTargetsSlug(l, target));
  return {
    content: kept.join("\n"),
    removed: kept.length !== lines.length,
  };
};

/** Replace the pointer line(s) for `slug` with `newLine`; append when none exist. */
export const updateIndexLine = (
  raw: string,
  slug: string,
  newLine: string
): string => {
  const target = normalizeSlug(slug);
  const lines = raw.split("\n");
  let replaced = false;
  const next = lines.map((l) => {
    if (lineTargetsSlug(l, target)) {
      replaced = true;
      return newLine;
    }
    return l;
  });
  return replaced ? next.join("\n") : insertIndexLine(raw, newLine);
};
