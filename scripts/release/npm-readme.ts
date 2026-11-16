/** Builds the npm wrapper README from the root README. Pure. */

const REPO = "Mark-Life/peektrace";
const CUT_HEADING = /^# Develop\s*$/mu;
const IMAGE_LINE = /^\s*!\[[^\]]*\]\([^)]*\)\s*$/gmu;
const TRAILING_RULE = /\n*---$/u;
const LEADING_DOT_SLASH = /^\.?\//u;
const RELATIVE_LINK = /\]\((?!https?:|#|mailto:)(?<path>[^)\s]+)\)/gu;

/**
 * Keep the root README from the top up to `# Develop`, drop image lines
 * (they would point at repo assets), pin relative links to `ref`, and
 * append a Source/License footer.
 */
export const npmReadme = (rootMd: string, ref: string) => {
  const cut = rootMd.search(CUT_HEADING);
  const head = cut === -1 ? rootMd : rootMd.slice(0, cut);

  const body = head
    .replaceAll(IMAGE_LINE, "")
    .replaceAll(
      RELATIVE_LINK,
      (_m, path: string) =>
        `](https://github.com/${REPO}/blob/${ref}/${path.replace(LEADING_DOT_SLASH, "")})`
    )
    .replaceAll(/\n{3,}/gu, "\n\n")
    .trimEnd()
    .replace(TRAILING_RULE, "");

  return `${body}\n\n---\n\nSource: https://github.com/${REPO}\n\nLicense: MIT\n`;
};
