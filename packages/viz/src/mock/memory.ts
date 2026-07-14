/** Illustrative sample memory-vault data for demos, docs, and the marketing site.
 *
 * Nothing here was captured from a real vault: it is a hand-authored vault that
 * exercises every state the memory views can render — indexed files, orphan
 * files absent from `MEMORY.md`, dangling links whose target file does not
 * exist, graph orphans with no links at all, and an index that is over budget on
 * both the 200-line and 25 KB cliffs. Values are typed against the core schemas,
 * so a schema change breaks this file rather than the demo.
 */
import type {
  GraphData,
  GraphNode,
  IndexBudget,
  LinkEdge,
} from "@workspace/core/services/memory/types";
import {
  MAX_INDEX_BYTES,
  MAX_INDEX_LINES,
} from "@workspace/core/services/memory/types";

/** Synthetic source node for index edges, matching core's graph builder. */
const INDEX_NODE = "__index__";

/**
 * Nine memory files: six reachable from the index, three orphaned outside it.
 * `bytes` drives node radius, `inIndex` the emerald/amber fill, and the degree
 * counts mirror what core's graph builder derives from {@link MOCK_EDGES}.
 */
const MOCK_NODES: readonly GraphNode[] = [
  {
    slug: "peektrace-rebrand",
    type: "project",
    bytes: 4812,
    inIndex: true,
    inDeg: 0,
    outDeg: 2,
  },
  {
    slug: "session-parsing",
    type: "reference",
    bytes: 3940,
    inIndex: true,
    inDeg: 2,
    outDeg: 1,
  },
  {
    slug: "inspector-shell",
    type: "project",
    bytes: 2610,
    inIndex: true,
    inDeg: 1,
    outDeg: 1,
  },
  {
    slug: "native-installer",
    type: "project",
    bytes: 3120,
    inIndex: true,
    inDeg: 1,
    outDeg: 1,
  },
  {
    slug: "release-blockers",
    type: "feedback",
    bytes: 2040,
    inIndex: true,
    inDeg: 2,
    outDeg: 0,
  },
  {
    slug: "user-preferences",
    type: "user",
    bytes: 1450,
    inIndex: true,
    inDeg: 0,
    outDeg: 0,
  },
  {
    slug: "codex-support",
    type: "reference",
    bytes: 5380,
    inIndex: false,
    inDeg: 1,
    outDeg: 1,
  },
  {
    slug: "pi-sessions",
    type: "reference",
    bytes: 1760,
    inIndex: false,
    inDeg: 0,
    outDeg: 1,
  },
  {
    slug: "old-scratch-notes",
    type: "unknown",
    bytes: 980,
    inIndex: false,
    inDeg: 0,
    outDeg: 0,
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
    to: "session-parsing",
    kind: "wiki",
    resolved: true,
    resolvedTo: "session-parsing",
    line: 12,
  },
  {
    from: "peektrace-rebrand",
    to: "inspector-shell",
    kind: "wiki",
    resolved: true,
    resolvedTo: "inspector-shell",
    line: 18,
  },
  {
    from: "peektrace-rebrand",
    to: "peektrace-brand",
    kind: "markdown",
    resolved: false,
    candidates: ["peektrace-rebrand"],
    line: 26,
  },
  {
    from: "session-parsing",
    to: "native-installer",
    kind: "markdown",
    resolved: true,
    resolvedTo: "native-installer",
    line: 7,
  },
  {
    from: "inspector-shell",
    to: "release-blockers",
    kind: "wiki",
    resolved: true,
    resolvedTo: "release-blockers",
    line: 22,
  },
  {
    from: "native-installer",
    to: "release-blockers",
    kind: "wiki",
    resolved: true,
    resolvedTo: "release-blockers",
    line: 9,
  },
  {
    from: "release-blockers",
    to: "archive-2024-notes",
    kind: "wiki",
    resolved: false,
    candidates: ["old-scratch-notes"],
    line: 31,
  },
  {
    from: "codex-support",
    to: "session-parsing",
    kind: "wiki",
    resolved: true,
    resolvedTo: "session-parsing",
    line: 5,
  },
  {
    from: "pi-sessions",
    to: "codex-support",
    kind: "wiki",
    resolved: true,
    resolvedTo: "codex-support",
    line: 4,
  },
  {
    from: INDEX_NODE,
    to: "peektrace-rebrand",
    kind: "index",
    resolved: true,
    resolvedTo: "peektrace-rebrand",
    line: 6,
  },
  {
    from: INDEX_NODE,
    to: "session-parsing",
    kind: "index",
    resolved: true,
    resolvedTo: "session-parsing",
    line: 7,
  },
  {
    from: INDEX_NODE,
    to: "inspector-shell",
    kind: "index",
    resolved: true,
    resolvedTo: "inspector-shell",
    line: 8,
  },
  {
    from: INDEX_NODE,
    to: "native-installer",
    kind: "index",
    resolved: true,
    resolvedTo: "native-installer",
    line: 9,
  },
  {
    from: INDEX_NODE,
    to: "release-blockers",
    kind: "index",
    resolved: true,
    resolvedTo: "release-blockers",
    line: 10,
  },
  {
    from: INDEX_NODE,
    to: "user-preferences",
    kind: "index",
    resolved: true,
    resolvedTo: "user-preferences",
    line: 11,
  },
  {
    from: INDEX_NODE,
    to: "agent-rules",
    kind: "index",
    resolved: false,
    candidates: [],
    line: 218,
  },
];

/**
 * Illustrative sample link graph for a nine-file vault. Amber nodes
 * (`inIndex: false`) are files the index never points at; `orphans` are the
 * files with no body links in either direction.
 */
export const MOCK_GRAPH: typeof GraphData.Type = {
  nodes: MOCK_NODES,
  edges: MOCK_EDGES,
  orphans: ["user-preferences", "old-scratch-notes"],
};

/**
 * Illustrative sample index budget: over budget on both cliffs (241 of 200
 * lines, 29,594 of 25,600 bytes), leaving six entries below the fold and
 * therefore never loaded into context.
 */
export const MOCK_INDEX_BUDGET: typeof IndexBudget.Type = {
  lines: 241,
  maxLines: MAX_INDEX_LINES,
  bytes: 29_594,
  maxBytes: MAX_INDEX_BYTES,
  overBudget: true,
  belowFoldCount: 6,
  kind: "index",
};

/**
 * Illustrative sample file counts per frontmatter `type`, matching the types on
 * {@link MOCK_GRAPH}'s nodes. `unknown` is the key core uses for files with no
 * `type` in their frontmatter.
 */
export const MOCK_TYPE_COUNTS: Readonly<Record<string, number>> = {
  project: 3,
  reference: 3,
  feedback: 1,
  user: 1,
  unknown: 1,
};
