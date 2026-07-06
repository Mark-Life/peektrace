/** Project-label helpers ported from `memory-view/scripts/lib/resolve.ts`.
 *
 * Project discovery itself reuses `AgentRegistry` (encodeSlug / listProjectSlugs);
 * only the human-readable labelling logic lives here.
 */

const LEADING_DASH_RE = /^-/;

/** Longest common leading substring across slugs (the shared home prefix). */
const commonPrefix = (slugs: readonly string[]): string => {
  let pre = slugs[0];
  if (pre === undefined) {
    return "";
  }
  for (const s of slugs) {
    let i = 0;
    while (i < pre.length && i < s.length && pre[i] === s[i]) {
      i++;
    }
    pre = pre.slice(0, i);
    if (!pre) {
      break;
    }
  }
  return pre;
};

/** Strip the shared home prefix for a readable label; fall back to the raw slug. */
const prettyLabel = (slug: string, prefix: string): string => {
  const stripped =
    prefix && slug.startsWith(prefix)
      ? slug.slice(prefix.length)
      : slug.replace(LEADING_DASH_RE, "");
  return stripped || slug;
};

/** Build a slug -> human label map by stripping the shared prefix across slugs. */
export const labelSlugs = (
  slugs: readonly string[]
): ReadonlyMap<string, string> => {
  const prefix = commonPrefix(slugs);
  return new Map(slugs.map((slug) => [slug, prettyLabel(slug, prefix)]));
};
