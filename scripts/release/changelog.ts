/**
 * Pure CHANGELOG.md helpers for the release workflow.
 * No Bun or Node APIs.
 */

const HEADING = /^## (?<tag>\S+)(?:\s+[—–-]\s+(?<date>\d{4}-\d{2}-\d{2}))?/u;
const NEWLINE = /\r?\n/u;
const LEADING_DOT_SLASH = /^\.?\//u;
const STARTS_WITH_DIGIT = /^\d/u;
const HEADING_PREFIX = "## ";
const FENCE = /^\s*(?<marker>`{3,}|~{3,})/u;
const INLINE_CODE = /`[^`]*`/gu;

const RELATIVE_LINK =
  /(?<bang>!?)\[(?<text>[^\]]*)\]\((?!https?:|#|mailto:|\/\/)(?<path>[^)\s]+)\)/gu;

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
  const marker = FENCE.exec(text)?.groups?.marker;

  if (marker === undefined || marker === "") {
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

  for (const [line, text] of lines.entries()) {
    const wasFenced = fence !== undefined;
    fence = nextFence(text, fence);

    if (wasFenced || fence !== undefined) {
      continue;
    }

    if (!text.startsWith(HEADING_PREFIX)) {
      continue;
    }

    const match = HEADING.exec(text);
    found.push({
      date: match?.groups?.date,
      line,
      tag: match?.groups?.tag ?? text.slice(HEADING_PREFIX.length).trim(),
    });
  }

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

  return line.replaceAll(
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
    return start;
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
  const found = headings(markdown.split(NEWLINE)).find(
    ({ tag }) =>
      tag.startsWith(prefix) && STARTS_WITH_DIGIT.test(tag.slice(prefix.length))
  );

  return (
    found && {
      date: found.date,
      tag: found.tag,
      version: found.tag.slice(prefix.length),
    }
  );
};
