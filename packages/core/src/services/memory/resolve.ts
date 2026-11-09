/** Project-label helpers ported from `memory-view/scripts/lib/resolve.ts`.
 *
 * Project discovery itself reuses `AgentRegistry` (encodeSlug / listProjectSlugs);
 * only the human-readable labelling logic lives here.
 */

const SEP = "-";
const LEADING_DASH_RE = /^-/;

/**
 * Shared leading `-` segments across slugs (the home prefix), always leaving
 * at least the last segment of the shortest slug, so `app` / `api` never
 * shrink to `p` / `i`. A lone slug has no prefix: a `-` in its folder name
 * looks the same as a path separator, so any cut could split the name.
 */
const commonPrefix = (slugs: readonly string[]) => {
  const parts = slugs.map((s) => s.split(SEP));
  const first = parts[0];
  if (first === undefined || parts.length < 2) {
    return "";
  }
  const limit = Math.min(...parts.map((p) => p.length)) - 1;
  let n = 0;
  while (n < limit && parts.every((p) => p[n] === first[n])) {
    n++;
  }
  return n === 0 ? "" : `${first.slice(0, n).join(SEP)}${SEP}`;
};

/** Strip the shared home prefix for a readable label; fall back to the raw slug. */
const prettyLabel = (slug: string, prefix: string) => {
  const stripped = prefix
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
