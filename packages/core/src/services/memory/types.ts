/** Schemas + derived types for the Claude memory read model and CRUD surface.
 *
 * Ported from `memory-view/scripts/lib/types.ts` and reshaped as `effect/Schema`
 * so every payload is serializable end-to-end (disk -> core -> RPC -> UI) with no
 * hand-duplicated TypeScript. Derive TS types via `typeof X.Type`.
 */
import { Schema } from "effect";

/** MEMORY.md is always-loaded; only the first 200 lines / 25 KB are visible. */
export const MAX_INDEX_LINES = 200;
/** Byte cliff for the always-loaded index (25 KB = 25 * 1024). */
export const MAX_INDEX_BYTES = 25_600;

/** Documented frontmatter `type` values; anything else is invalid (SCH03). */
export const MEMORY_TYPES = [
  "user",
  "feedback",
  "project",
  "reference",
] as const;

/** Documented memory `type` literal. */
export const MemoryType = Schema.Literal(...MEMORY_TYPES);
export type MemoryType = typeof MemoryType.Type;

/** Runtime set form for membership checks. */
export const VALID_TYPES: ReadonlySet<string> = new Set(MEMORY_TYPES);

/** A kebab-case memory name, branded once validated at the boundary. */
export const MemoryName = Schema.String.pipe(
  Schema.pattern(/^[a-z0-9]+(?:-[a-z0-9]+)*$/),
  Schema.brand("MemoryName")
);
export type MemoryName = typeof MemoryName.Type;

/** Frontmatter shape variant observed on disk. */
export const SchemaShape = Schema.Literal("nested", "flat", "none");
export type SchemaShape = typeof SchemaShape.Type;

/**
 * Parsed YAML frontmatter, tolerant of the three on-disk shapes (nested
 * `metadata:` block, legacy flat, or none). `raw` preserves the exact block text
 * so a write re-emits unchanged frontmatter byte-for-byte.
 */
export const Frontmatter = Schema.Struct({
  raw: Schema.String,
  shape: SchemaShape,
  name: Schema.optional(Schema.String),
  description: Schema.optional(Schema.String),
  type: Schema.optional(Schema.String),
  nodeType: Schema.optional(Schema.String),
  originSessionId: Schema.optional(Schema.String),
  created: Schema.optional(Schema.String),
  updated: Schema.optional(Schema.String),
  extra: Schema.Record({ key: Schema.String, value: Schema.String }),
  hadTrailingMetadataWs: Schema.Boolean,
  descriptionQuoted: Schema.Boolean,
});
export type Frontmatter = typeof Frontmatter.Type;

/** Outbound link style found in a body. */
export const LinkKind = Schema.Literal("wiki", "markdown");
export type LinkKind = typeof LinkKind.Type;

/** A single outbound link parsed from a memory body. */
export const LinkRef = Schema.Struct({
  kind: LinkKind,
  rawTarget: Schema.String,
  targetSlug: Schema.String,
  line: Schema.Number,
});
export type LinkRef = typeof LinkRef.Type;

/** One memory topic file, fully parsed and ready to render or edit. */
export const MemoryEntry = Schema.Struct({
  slug: Schema.String,
  fileName: Schema.String,
  name: Schema.optional(Schema.String),
  description: Schema.optional(Schema.String),
  type: Schema.optional(Schema.String),
  body: Schema.String,
  size: Schema.Number,
  lines: Schema.Number,
  /** mtime as ISO string for display. */
  mtime: Schema.String,
  /** mtime in epoch ms for compare-and-swap. */
  mtimeMs: Schema.Number,
  bodyHash: Schema.String,
  inIndex: Schema.Boolean,
  links: Schema.Array(LinkRef),
  frontmatter: Frontmatter,
});
export type MemoryEntry = typeof MemoryEntry.Type;

/** One parsed line of the MEMORY.md index. */
export const IndexEntry = Schema.Struct({
  raw: Schema.String,
  lineNumber: Schema.Number,
  label: Schema.optional(Schema.String),
  target: Schema.optional(Schema.String),
  targetSlug: Schema.optional(Schema.String),
  hook: Schema.optional(Schema.String),
  malformed: Schema.Boolean,
  kind: Schema.Literal("markdown", "wiki", "bare"),
});
export type IndexEntry = typeof IndexEntry.Type;

/** The MEMORY.md index file (or its absence). */
export const MemoryIndex = Schema.Struct({
  exists: Schema.Boolean,
  path: Schema.String,
  raw: Schema.String,
  bytes: Schema.Number,
  lines: Schema.Number,
  entries: Schema.Array(IndexEntry),
  isMonolithic: Schema.Boolean,
  overBudget: Schema.Boolean,
  belowFoldEntries: Schema.Array(IndexEntry),
});
export type MemoryIndex = typeof MemoryIndex.Type;

/** Index budget summary — the headline gauge. */
export const IndexBudget = Schema.Struct({
  lines: Schema.Number,
  maxLines: Schema.Number,
  bytes: Schema.Number,
  maxBytes: Schema.Number,
  overBudget: Schema.Boolean,
  belowFoldCount: Schema.Number,
  kind: Schema.Literal("index", "monolithic", "absent"),
});
export type IndexBudget = typeof IndexBudget.Type;

/** Per-node graph summary for the arc view. */
export const GraphNode = Schema.Struct({
  slug: Schema.String,
  type: Schema.String,
  bytes: Schema.Number,
  inIndex: Schema.Boolean,
  inDeg: Schema.Number,
  outDeg: Schema.Number,
});
export type GraphNode = typeof GraphNode.Type;

/** A resolved edge in the link graph (body links + index edges). */
export const LinkEdge = Schema.Struct({
  from: Schema.String,
  to: Schema.String,
  kind: Schema.Literal("wiki", "markdown", "index"),
  resolved: Schema.Boolean,
  resolvedTo: Schema.optional(Schema.String),
  ambiguous: Schema.optional(Schema.Boolean),
  candidates: Schema.optional(Schema.Array(Schema.String)),
  line: Schema.optional(Schema.Number),
});
export type LinkEdge = typeof LinkEdge.Type;

/** Resolved link graph over a vault. */
export const GraphData = Schema.Struct({
  nodes: Schema.Array(GraphNode),
  edges: Schema.Array(LinkEdge),
  orphans: Schema.Array(Schema.String),
});
export type GraphData = typeof GraphData.Type;

/** A dangling index reference (entry whose target file is missing). */
export const DanglingEntry = Schema.Struct({
  target: Schema.String,
  line: Schema.Number,
});
export type DanglingEntry = typeof DanglingEntry.Type;

/** Index<->files diff: files not indexed vs entries with no file. */
export const VaultDiff = Schema.Struct({
  orphans: Schema.Array(Schema.String),
  dangling: Schema.Array(DanglingEntry),
});
export type VaultDiff = typeof VaultDiff.Type;

/** Vault-level state classification. */
export const VaultState = Schema.Literal("ok", "absent", "empty", "monolithic");
export type VaultState = typeof VaultState.Type;

/** The whole parsed vault for one project. */
export const MemoryVault = Schema.Struct({
  slug: Schema.String,
  project: Schema.String,
  memoryDir: Schema.String,
  state: VaultState,
  index: Schema.NullOr(MemoryIndex),
  entries: Schema.Array(MemoryEntry),
  strayFiles: Schema.Array(Schema.String),
  budget: IndexBudget,
  graph: GraphData,
  diff: VaultDiff,
  typeCounts: Schema.Record({ key: Schema.String, value: Schema.Number }),
  totalBytes: Schema.Number,
});
export type MemoryVault = typeof MemoryVault.Type;

/** A discovered project that has a non-empty memory dir. */
export const ProjectSummary = Schema.Struct({
  slug: Schema.String,
  project: Schema.String,
  memoryDir: Schema.String,
  fileCount: Schema.Number,
  hasIndex: Schema.Boolean,
});
export type ProjectSummary = typeof ProjectSummary.Type;

/** The cross-project default: project overview + every vault. */
export const AllVaults = Schema.Struct({
  projects: Schema.Array(ProjectSummary),
  vaults: Schema.Array(MemoryVault),
});
export type AllVaults = typeof AllVaults.Type;

/** A body reference left dangling after a delete. */
export const DanglingRef = Schema.Struct({
  from: Schema.String,
  target: Schema.String,
  line: Schema.optional(Schema.Number),
});
export type DanglingRef = typeof DanglingRef.Type;

/** Result of a delete: the references it leaves broken. */
export const DeleteResult = Schema.Struct({
  slug: Schema.String,
  dangling: Schema.Array(DanglingRef),
});
export type DeleteResult = typeof DeleteResult.Type;
