/** Illustrative sample memory-vault data for demos, docs, and the marketing site.
 *
 * Nothing here was captured from a real vault: it is a hand-authored vault that
 * exercises every state the memory views can render — indexed files, orphan
 * files absent from `MEMORY.md`, dangling links whose target file does not
 * exist, graph orphans with no links at all, and an index that is over budget on
 * both the 200-line and 25 KB cliffs. Values are typed against the generated wire types,
 * so a schema change breaks this file rather than the demo.
 */
import type {
  GraphData,
  GraphNode,
  IndexBudget,
  LinkEdge,
} from "@workspace/view-core/api-types.gen";
import {
  MAX_INDEX_BYTES,
  MAX_INDEX_LINES,
} from "@workspace/view-core/constants.gen";

/** Synthetic source node for index edges, matching core's graph builder. */
const INDEX_NODE = "__index__";

/**
 * Nine memory files: six reachable from the index, three orphaned outside it.
 * `bytes` drives node radius, `inIndex` the emerald/amber fill, and the degree
 * counts mirror what core's graph builder derives from {@link MOCK_EDGES}.
 */
const MOCK_NODES: readonly GraphNode[] = [
  {
    bytes: 4812,
    inDeg: 0,
    inIndex: true,
    outDeg: 2,
    slug: "peektrace-rebrand",
    type: "project",
  },
  {
    bytes: 3940,
    inDeg: 2,
    inIndex: true,
    outDeg: 1,
    slug: "session-parsing",
    type: "reference",
  },
  {
    bytes: 2610,
    inDeg: 1,
    inIndex: true,
    outDeg: 1,
    slug: "inspector-shell",
    type: "project",
  },
  {
    bytes: 3120,
    inDeg: 1,
    inIndex: true,
    outDeg: 1,
    slug: "native-installer",
    type: "project",
  },
  {
    bytes: 2040,
    inDeg: 2,
    inIndex: true,
    outDeg: 0,
    slug: "release-blockers",
    type: "feedback",
  },
  {
    bytes: 1450,
    inDeg: 0,
    inIndex: true,
    outDeg: 0,
    slug: "user-preferences",
    type: "user",
  },
  {
    bytes: 5380,
    inDeg: 1,
    inIndex: false,
    outDeg: 1,
    slug: "codex-support",
    type: "reference",
  },
  {
    bytes: 1760,
    inDeg: 0,
    inIndex: false,
    outDeg: 1,
    slug: "pi-sessions",
    type: "reference",
  },
  {
    bytes: 980,
    inDeg: 0,
    inIndex: false,
    outDeg: 0,
    slug: "old-scratch-notes",
    type: "unknown",
  },
];

/**
 * Body links between files, plus the index's own pointers. Two body links are
 * unresolved: `archive-2024-notes` and `peektrace-brand` name files that were
 * renamed away, so each carries near-match repair `candidates`. The index also
 * points at one missing file (`agent-rules`).
 */
const MOCK_EDGES: readonly LinkEdge[] = [
  {
    from: "peektrace-rebrand",
    kind: "wiki",
    line: 12,
    resolved: true,
    resolvedTo: "session-parsing",
    to: "session-parsing",
  },
  {
    from: "peektrace-rebrand",
    kind: "wiki",
    line: 18,
    resolved: true,
    resolvedTo: "inspector-shell",
    to: "inspector-shell",
  },
  {
    candidates: ["peektrace-rebrand"],
    from: "peektrace-rebrand",
    kind: "markdown",
    line: 26,
    resolved: false,
    to: "peektrace-brand",
  },
  {
    from: "session-parsing",
    kind: "markdown",
    line: 7,
    resolved: true,
    resolvedTo: "native-installer",
    to: "native-installer",
  },
  {
    from: "inspector-shell",
    kind: "wiki",
    line: 22,
    resolved: true,
    resolvedTo: "release-blockers",
    to: "release-blockers",
  },
  {
    from: "native-installer",
    kind: "wiki",
    line: 9,
    resolved: true,
    resolvedTo: "release-blockers",
    to: "release-blockers",
  },
  {
    candidates: ["old-scratch-notes"],
    from: "release-blockers",
    kind: "wiki",
    line: 31,
    resolved: false,
    to: "archive-2024-notes",
  },
  {
    from: "codex-support",
    kind: "wiki",
    line: 5,
    resolved: true,
    resolvedTo: "session-parsing",
    to: "session-parsing",
  },
  {
    from: "pi-sessions",
    kind: "wiki",
    line: 4,
    resolved: true,
    resolvedTo: "codex-support",
    to: "codex-support",
  },
  {
    from: INDEX_NODE,
    kind: "index",
    line: 6,
    resolved: true,
    resolvedTo: "peektrace-rebrand",
    to: "peektrace-rebrand",
  },
  {
    from: INDEX_NODE,
    kind: "index",
    line: 7,
    resolved: true,
    resolvedTo: "session-parsing",
    to: "session-parsing",
  },
  {
    from: INDEX_NODE,
    kind: "index",
    line: 8,
    resolved: true,
    resolvedTo: "inspector-shell",
    to: "inspector-shell",
  },
  {
    from: INDEX_NODE,
    kind: "index",
    line: 9,
    resolved: true,
    resolvedTo: "native-installer",
    to: "native-installer",
  },
  {
    from: INDEX_NODE,
    kind: "index",
    line: 10,
    resolved: true,
    resolvedTo: "release-blockers",
    to: "release-blockers",
  },
  {
    from: INDEX_NODE,
    kind: "index",
    line: 11,
    resolved: true,
    resolvedTo: "user-preferences",
    to: "user-preferences",
  },
  {
    candidates: [],
    from: INDEX_NODE,
    kind: "index",
    line: 218,
    resolved: false,
    to: "agent-rules",
  },
];

/**
 * Illustrative sample link graph for a nine-file vault. Amber nodes
 * (`inIndex: false`) are files the index never points at; `orphans` are the
 * files with no body links in either direction.
 */
export const MOCK_GRAPH: GraphData = {
  edges: MOCK_EDGES,
  nodes: MOCK_NODES,
  orphans: ["user-preferences", "old-scratch-notes"],
};

/**
 * Illustrative sample index budget: over budget on both cliffs (241 of 200
 * lines, 29,594 of 25,600 bytes), leaving six entries below the fold and
 * therefore never loaded into context.
 */
export const MOCK_INDEX_BUDGET: IndexBudget = {
  belowFoldCount: 6,
  bytes: 29_594,
  kind: "index",
  lines: 241,
  maxBytes: MAX_INDEX_BYTES,
  maxLines: MAX_INDEX_LINES,
  overBudget: true,
};

/**
 * Illustrative sample file counts per frontmatter `type`, matching the types on
 * {@link MOCK_GRAPH}'s nodes. `unknown` is the key core uses for files with no
 * `type` in their frontmatter.
 */
export const MOCK_TYPE_COUNTS: Readonly<Record<string, number>> = {
  feedback: 1,
  project: 3,
  reference: 3,
  unknown: 1,
  user: 1,
};
