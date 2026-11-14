/** Builds the npm wrapper README from the root README. Pure. */

const REPO = "Mark-Life/peektrace";
const CUT_HEADING = /^# Develop\s*$/m;
const IMAGE_LINE = /^\s*!\[[^\]]*\]\([^)]*\)\s*$/gm;
const TRAILING_RULE = /\n*---$/;
const LEADING_DOT_SLASH = /^\.?\//;
const RELATIVE_LINK = /\]\((?!https?:|#|mailto:)([^)\s]+)\)/g;

/**
 * Keep the root README from the top up to `# Develop`, drop image lines
 * (they would point at repo assets), pin relative links to `ref`, and
 * append a Source/License footer.
 */
export const npmReadme = (rootMd: string, ref: string) => {
  const cut = rootMd.search(CUT_HEADING);
  const head = cut === -1 ? rootMd : rootMd.slice(0, cut);
  const body = head
    .replace(IMAGE_LINE, "")
    .replace(
      RELATIVE_LINK,
      (_m, path: string) =>
        `](https://github.com/${REPO}/blob/${ref}/${path.replace(LEADING_DOT_SLASH, "")})`
    )
    .replace(/\n{3,}/g, "\n\n")
    .trimEnd()
    .replace(TRAILING_RULE, "");
  return `${body}\n\n---\n\nSource: https://github.com/${REPO}\n\nLicense: MIT\n`;
};
