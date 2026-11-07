/**
 * Pure CHANGELOG.md helpers shared by the release workflow and the website.
 * No Bun or Node APIs: the web app imports this file directly.
 */

const HEADING = /^## (\S+)(?:\s+[—–-]\s+(\d{4}-\d{2}-\d{2}))?/;
const NEWLINE = /\r?\n/;
const LEADING_DOT_SLASH = /^\.?\//;
const STARTS_WITH_DIGIT = /^\d/;
const HEADING_PREFIX = "## ";
const FENCE = /^\s*(`{3,}|~{3,})/;
const INLINE_CODE = /`[^`]*`/g;
const RELATIVE_LINK =
  /(!?)\[([^\]]*)\]\((?!https?:|#|mailto:|\/\/)([^)\s]+)\)/g;

interface Heading {
  readonly date: string | undefined;
  readonly line: number;
  readonly tag: string;
}

interface Fence {
  readonly char: string;
  readonly size: number;
}

/** Next fence state after `text`: opens on a marker, closes on a matching one. */
const nextFence = (text: string, fence: Fence | undefined) => {
  const marker = FENCE.exec(text)?.[1];
  if (!marker) {
    return fence;
  }
  const next = { char: marker[0] ?? "", size: marker.length };
  if (!fence) {
    return next;
  }
  const closes = next.char === fence.char && next.size >= fence.size;
  return closes ? undefined : fence;
};

/** Level-2 headings outside code fences, in document order. */
const headings = (lines: readonly string[]): readonly Heading[] => {
  const found: Heading[] = [];
  let fence: Fence | undefined;
  lines.forEach((text, line) => {
    const wasFenced = fence !== undefined;
    fence = nextFence(text, fence);
    if (wasFenced || fence !== undefined) {
      return;
    }
    if (!text.startsWith(HEADING_PREFIX)) {
      return;
    }
    const match = HEADING.exec(text);
    found.push({
      line,
      tag: match?.[1] ?? text.slice(HEADING_PREFIX.length).trim(),
      date: match?.[2],
    });
  });
  return found;
};

export interface ChangelogLinks {
  readonly repoUrl: string;
  readonly tag: string;
}

const absolutizeLine = (line: string, { repoUrl, tag }: ChangelogLinks) => {
  const spans = [...line.matchAll(INLINE_CODE)].map(
    (match) => [match.index, match.index + match[0].length] as const
  );
  return line.replace(
    RELATIVE_LINK,
    (whole, bang: string, text: string, path: string, offset: number) =>
      spans.some(([from, to]) => offset >= from && offset < to)
        ? whole
        : `${bang}[${text}](${repoUrl}/${bang ? "raw" : "blob"}/${tag}/${path.replace(LEADING_DOT_SLASH, "")})`
  );
};

/** Rewrites relative links outside code fences and inline code spans. */
const absolutizeLinks = (body: string, links: ChangelogLinks) => {
  let fence: Fence | undefined;
  return body
    .split(NEWLINE)
    .map((line) => {
      const wasFenced = fence !== undefined;
      fence = nextFence(line, fence);
      return wasFenced || fence !== undefined
        ? line
        : absolutizeLine(line, links);
    })
    .join("\n");
};

/**
 * Body of the `## <tag>` section, up to the next `## ` heading. The tag must
 * match exactly. Pass `links` to turn relative markdown links absolute.
 */
export const changelogSection = (
  markdown: string,
  tag: string,
  links?: Omit<ChangelogLinks, "tag">
) => {
  const lines = markdown.split(NEWLINE);
  const all = headings(lines);
  const index = all.findIndex((heading) => heading.tag === tag);
  const start = all[index];
  if (!start) {
    return;
  }
  const end = all[index + 1]?.line ?? lines.length;
  const body = lines
    .slice(start.line + 1, end)
    .join("\n")
    .trim();
  return links ? absolutizeLinks(body, { ...links, tag }) : body;
};

/** First released heading (`## cli-vX.Y.Z — date`); skips `Unreleased`. */
export const latestRelease = (markdown: string, prefix = "cli-v") => {
  for (const { tag, date } of headings(markdown.split(NEWLINE))) {
    if (
      tag.startsWith(prefix) &&
      STARTS_WITH_DIGIT.test(tag.slice(prefix.length))
    ) {
      return { tag, version: tag.slice(prefix.length), date };
    }
  }
};
