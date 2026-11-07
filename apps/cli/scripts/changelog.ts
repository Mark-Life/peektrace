/**
 * Pure CHANGELOG.md helpers shared by the release workflow and the website.
 * No Bun or Node APIs: the web app imports this file directly.
 */

const HEADING = /^## (\S+)(?:\s+[—–-]\s+(\S+))?\s*$/;
const NEWLINE = /\r?\n/;
const LEADING_DOT_SLASH = /^\.?\//;
const STARTS_WITH_DIGIT = /^\d/;
const HEADING_PREFIX = "## ";
const FENCE = /^\s*(```|~~~)/;
const RELATIVE_LINK = /\]\((?!https?:|#|mailto:|\/\/)([^)\s]+)\)/g;

interface Heading {
  readonly date: string | undefined;
  readonly line: number;
  readonly tag: string;
}

/** Level-2 headings outside code fences, in document order. */
const headings = (lines: readonly string[]): readonly Heading[] => {
  const found: Heading[] = [];
  let fenced = false;
  lines.forEach((text, line) => {
    if (FENCE.test(text)) {
      fenced = !fenced;
      return;
    }
    if (fenced || !text.startsWith(HEADING_PREFIX)) {
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

const absolutizeLinks = (body: string, { repoUrl, tag }: ChangelogLinks) =>
  body.replace(
    RELATIVE_LINK,
    (_, path: string) =>
      `](${repoUrl}/blob/${tag}/${path.replace(LEADING_DOT_SLASH, "")})`
  );

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
