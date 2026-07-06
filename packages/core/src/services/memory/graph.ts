/** Build and resolve the memory link graph: body links + index edges against the
 * set of real files, flagging broken / ambiguous links and graph orphans, with
 * near-match repair candidates. Ported from `memory-view/scripts/lib/graph.ts`.
 */
import type {
  GraphData,
  GraphNode,
  IndexEntry,
  LinkEdge,
  MemoryEntry,
} from "./types";

/** Synthetic node id for index-originated edges. */
export const INDEX_NODE = "__index__";

const MAX_LEN_DELTA = 3;
const NEAR_MATCH_DISTANCE = 2;
const UNREACHABLE_DISTANCE = 99;

/** Levenshtein distance, capped — cheap on the tiny target sets here. */
const editDistance = (a: string, b: string): number => {
  const m = a.length;
  const n = b.length;
  if (Math.abs(m - n) > MAX_LEN_DELTA) {
    return UNREACHABLE_DISTANCE;
  }
  let prev = Array.from({ length: n + 1 }, (_, j) => j);
  for (let i = 1; i <= m; i++) {
    const cur = new Array<number>(n + 1).fill(0);
    cur[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      const del = (prev[j] ?? 0) + 1;
      const ins = (cur[j - 1] ?? 0) + 1;
      const sub = (prev[j - 1] ?? 0) + cost;
      cur[j] = Math.min(del, ins, sub);
    }
    prev = cur;
  }
  return prev[n] ?? 0;
};

/** Unique near-matches for an unresolved target slug (for fix-link repair). */
const nearMatches = (target: string, slugs: readonly string[]): string[] =>
  slugs
    .filter((s) => s !== target)
    .filter(
      (s) =>
        s.includes(target) ||
        target.includes(s) ||
        editDistance(target, s) <= NEAR_MATCH_DISTANCE
    )
    .sort((a, b) => editDistance(target, a) - editDistance(target, b));

/** Resolve a referenced slug against the file set (case-folded), flagging ambiguity. */
const makeResolver = (entries: readonly MemoryEntry[]) => {
  const byNorm = new Map<string, string[]>();
  for (const f of entries) {
    const k = f.slug.toLowerCase();
    byNorm.set(k, [...(byNorm.get(k) ?? []), f.slug]);
  }
  return (target: string): { resolvedTo?: string; ambiguous: boolean } => {
    const hits = byNorm.get(target) ?? [];
    const first = hits[0];
    if (first !== undefined && hits.length === 1) {
      return { resolvedTo: first, ambiguous: false };
    }
    if (first !== undefined && hits.length > 1) {
      return { resolvedTo: first, ambiguous: true };
    }
    return { ambiguous: false };
  };
};

type Resolver = ReturnType<typeof makeResolver>;

/** Bump an in/out degree map by one for `key`. */
const bump = (map: Map<string, number>, key: string): void => {
  map.set(key, (map.get(key) ?? 0) + 1);
};

/** Build one resolved body edge from a source file's link. */
const makeBodyEdge = (args: {
  readonly fromSlug: string;
  readonly targetSlug: string;
  readonly kind: LinkEdge["kind"];
  readonly line: number;
  readonly resolveSlug: Resolver;
  readonly fileSlugs: readonly string[];
}): LinkEdge => {
  const { resolvedTo, ambiguous } = args.resolveSlug(args.targetSlug);
  return {
    from: args.fromSlug,
    to: args.targetSlug,
    kind: args.kind,
    resolved: resolvedTo !== undefined,
    ...(resolvedTo === undefined ? {} : { resolvedTo }),
    ...(ambiguous ? { ambiguous: true } : {}),
    ...(resolvedTo === undefined
      ? { candidates: nearMatches(args.targetSlug, args.fileSlugs) }
      : {}),
    line: args.line,
  };
};

/** Body link edges + the in/out degree maps they imply. */
const bodyEdges = (args: {
  readonly entries: readonly MemoryEntry[];
  readonly resolveSlug: Resolver;
  readonly fileSlugs: readonly string[];
}): {
  edges: LinkEdge[];
  inDeg: Map<string, number>;
  outDeg: Map<string, number>;
} => {
  const { entries, resolveSlug, fileSlugs } = args;
  const edges: LinkEdge[] = [];
  const inDeg = new Map<string, number>();
  const outDeg = new Map<string, number>();
  const links = entries.flatMap((f) =>
    f.links.map((link) => ({ from: f.slug, link }))
  );
  for (const { from, link } of links) {
    const edge = makeBodyEdge({
      fromSlug: from,
      targetSlug: link.targetSlug,
      kind: link.kind,
      line: link.line,
      resolveSlug,
      fileSlugs,
    });
    edges.push(edge);
    if (edge.resolvedTo !== undefined) {
      bump(outDeg, from);
      if (edge.resolvedTo !== from) {
        bump(inDeg, edge.resolvedTo);
      }
    }
  }
  return { edges, inDeg, outDeg };
};

/** Index edges + the set of file slugs reached by the index. */
const indexEdges = (args: {
  readonly indexEntries: readonly IndexEntry[];
  readonly resolveSlug: Resolver;
  readonly fileSlugs: readonly string[];
}): { edges: LinkEdge[]; indexed: Set<string> } => {
  const { indexEntries, resolveSlug, fileSlugs } = args;
  const edges: LinkEdge[] = [];
  const indexed = new Set<string>();
  for (const e of indexEntries) {
    if (!e.targetSlug) {
      continue;
    }
    const { resolvedTo } = resolveSlug(e.targetSlug);
    if (resolvedTo) {
      indexed.add(resolvedTo);
    }
    edges.push({
      from: INDEX_NODE,
      to: e.targetSlug,
      kind: "index",
      resolved: resolvedTo !== undefined,
      ...(resolvedTo === undefined ? {} : { resolvedTo }),
      ...(resolvedTo === undefined
        ? { candidates: nearMatches(e.targetSlug, fileSlugs) }
        : {}),
      line: e.lineNumber,
    });
  }
  return { edges, indexed };
};

/**
 * Resolve all edges in the vault. Body links contribute to node degrees and
 * orphan detection; index edges are tracked for dangling-entry checks but do not
 * count toward graph orphan status.
 */
export const buildGraph = (args: {
  readonly entries: readonly MemoryEntry[];
  readonly indexEntries: readonly IndexEntry[];
}): GraphData => {
  const { entries, indexEntries } = args;
  const fileSlugs = entries.map((f) => f.slug);
  const resolveSlug = makeResolver(entries);

  const body = bodyEdges({ entries, resolveSlug, fileSlugs });
  const index = indexEdges({ indexEntries, resolveSlug, fileSlugs });

  const nodes: GraphNode[] = entries.map((f) => ({
    slug: f.slug,
    type: f.frontmatter.type ?? "unknown",
    bytes: f.size,
    inIndex: index.indexed.has(f.slug),
    inDeg: body.inDeg.get(f.slug) ?? 0,
    outDeg: body.outDeg.get(f.slug) ?? 0,
  }));

  const orphans = nodes
    .filter((n) => n.inDeg === 0 && n.outDeg === 0)
    .map((n) => n.slug);

  return { nodes, edges: [...body.edges, ...index.edges], orphans };
};
