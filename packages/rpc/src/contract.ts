/** Effect-RPC contract for Peektrace.
 *
 * One `RpcGroup` over the core services. Every success/payload shape is derived
 * from the core `effect/Schema` definitions (imported from `@workspace/core`) so
 * the wire types flow disk -> core -> client with no hand-duplication. The only
 * locally-defined schemas are (a) thin request payloads that have no core schema
 * (small filter/arg structs) and (b) `Schema.TaggedError` mirrors of the core
 * `Data.TaggedError` domain errors, required because `@effect/rpc` needs a
 * `Schema` error channel. The mirrors carry identical `_tag` + fields, so a
 * handler maps a core error to its wire twin field-for-field.
 */

import { Rpc, RpcGroup } from "@effect/rpc";
// Import from node-free deep paths (not the `@workspace/core` barrel) so this
// contract — and the browser bundle that pulls it in — never loads core's
// `node:*` resolvers (see `services/agent-id.ts`).
import { AgentId } from "@workspace/core/services/agent-id";
import {
  AllVaults,
  DeleteResult,
  MemoryEntry,
  MemoryVault,
  ProjectSummary,
} from "@workspace/core/services/memory/types";
import {
  AnalyzedSession,
  ParsedSession,
  SessionHeader,
} from "@workspace/core/services/sessions/schema";
import { Schema } from "effect";

// --- Wire error mirrors (Schema.TaggedError twins of the core Data errors) ---

/** Wire twin of core `FileChangedError` (compare-and-swap conflict). */
export class FileChangedError extends Schema.TaggedError<FileChangedError>()(
  "FileChangedError",
  {
    path: Schema.String,
    reason: Schema.Literal("mtime", "hash", "missing"),
  }
) {}

/** Wire twin of core `PathOutsideRootError` (write escaped the agent roots). */
export class PathOutsideRootError extends Schema.TaggedError<PathOutsideRootError>()(
  "PathOutsideRootError",
  {
    path: Schema.String,
    roots: Schema.Array(Schema.String),
  }
) {}

/** Wire twin of core `CapabilityUnsupportedError` (write gated by the matrix). */
export class CapabilityUnsupportedError extends Schema.TaggedError<CapabilityUnsupportedError>()(
  "CapabilityUnsupportedError",
  {
    capabilityId: Schema.String,
    agentId: AgentId,
  }
) {}

/** Wire twin of core `MemoryValidationError` (bad name/type/duplicate). */
export class MemoryValidationError extends Schema.TaggedError<MemoryValidationError>()(
  "MemoryValidationError",
  {
    reason: Schema.String,
    name: Schema.optional(Schema.String),
  }
) {}

/** Wire twin of core `MemoryNotFoundError`. */
export class MemoryNotFoundError extends Schema.TaggedError<MemoryNotFoundError>()(
  "MemoryNotFoundError",
  {
    project: Schema.String,
    name: Schema.String,
  }
) {}

/** Wire twin of core `SessionNotFoundError`. */
export class SessionNotFoundError extends Schema.TaggedError<SessionNotFoundError>()(
  "SessionNotFoundError",
  {
    id: Schema.String,
    searchedRoot: Schema.String,
  }
) {}

/** Wire twin of core `TranscriptParseError`. */
export class TranscriptParseError extends Schema.TaggedError<TranscriptParseError>()(
  "TranscriptParseError",
  {
    path: Schema.String,
    reason: Schema.String,
  }
) {}

/** Union of every wire error a fallible procedure can surface. */
export const WireError = Schema.Union(
  FileChangedError,
  PathOutsideRootError,
  CapabilityUnsupportedError,
  MemoryValidationError,
  MemoryNotFoundError,
  SessionNotFoundError,
  TranscriptParseError
);
export type WireError = typeof WireError.Type;

// --- Capability schema (the core matrix is a TS interface, not a Schema) ---

/** Support level mirror for the capability matrix. */
export const SupportLevel = Schema.Literal(
  "supported",
  "partial",
  "planned",
  "unsupported"
);

/** Per-agent support cell. */
export const CapabilitySupport = Schema.Struct({
  level: SupportLevel,
  note: Schema.optional(Schema.String),
});

/** One feature row in the matrix; `perAgent` is exhaustive over `AgentId`. */
export const Capability = Schema.Struct({
  id: Schema.String,
  group: Schema.String,
  title: Schema.String,
  description: Schema.String,
  perAgent: Schema.Record({ key: AgentId, value: CapabilitySupport }),
});
export type Capability = typeof Capability.Type;

// --- Watch (filesystem-driven freshness) ---

/**
 * Monotonic per-scope version counters returned by `watch.poll`. The client
 * polls on an interval; an increase in a scope means "refetch that scope"
 * (memory list/gauge, or the session list). The core `WatchService` advances
 * these from the real `@effect/platform` file watcher, coalescing bursts.
 */
export const WatchVersions = Schema.Struct({
  memory: Schema.Number,
  sessions: Schema.Number,
});
export type WatchVersions = typeof WatchVersions.Type;

// --- Request payloads with no core schema (small filter/arg structs) ---

/** `sessions.list` filter. */
export const SessionsListPayload = Schema.Struct({
  project: Schema.optional(Schema.String),
  agent: Schema.optional(AgentId),
});

/** `sessions.get` arguments. */
export const SessionGetPayload = Schema.Struct({
  id: Schema.String,
  redact: Schema.optional(Schema.Boolean),
});

/** `sessions.analyze` arguments. */
export const SessionAnalyzePayload = Schema.Struct({
  id: Schema.String,
  window: Schema.optional(Schema.Number),
  dumbZone: Schema.optional(Schema.Number),
  redact: Schema.optional(Schema.Boolean),
});

/** `memory.vault` selector. */
export const MemoryVaultPayload = Schema.Struct({
  project: Schema.String,
});

/** `memory.create` arguments. */
export const MemoryCreatePayload = Schema.Struct({
  project: Schema.String,
  name: Schema.String,
  description: Schema.String,
  type: Schema.String,
  body: Schema.String,
});

/** Editable frontmatter subset for `memory.update`. */
export const MemoryFrontmatterPatch = Schema.Struct({
  name: Schema.optional(Schema.String),
  description: Schema.optional(Schema.String),
  type: Schema.optional(Schema.String),
});

/** `memory.update` arguments (CAS on `expectedMtime`). */
export const MemoryUpdatePayload = Schema.Struct({
  project: Schema.String,
  name: Schema.String,
  frontmatter: Schema.optional(MemoryFrontmatterPatch),
  body: Schema.optional(Schema.String),
  expectedMtime: Schema.optional(Schema.Number),
});

/** `memory.delete` selector. */
export const MemoryDeletePayload = Schema.Struct({
  project: Schema.String,
  name: Schema.String,
});

// --- The RPC group ---

/**
 * The single typed Peektrace RPC surface. Tags are dotted (`group.method`) so the
 * generated client nests as `client.sessions.analyze(...)` etc.
 */
export const PeektraceRpcs = RpcGroup.make(
  Rpc.make("capabilities.list", {
    success: Schema.Array(Capability),
  }),
  Rpc.make("sessions.list", {
    payload: SessionsListPayload,
    success: Schema.Array(SessionHeader),
  }),
  Rpc.make("sessions.get", {
    payload: SessionGetPayload,
    success: ParsedSession,
    error: WireError,
  }),
  Rpc.make("sessions.analyze", {
    payload: SessionAnalyzePayload,
    success: AnalyzedSession,
    error: WireError,
  }),
  Rpc.make("memory.allVaults", {
    success: AllVaults,
  }),
  Rpc.make("memory.projects", {
    success: Schema.Array(ProjectSummary),
  }),
  Rpc.make("memory.vault", {
    payload: MemoryVaultPayload,
    success: MemoryVault,
  }),
  Rpc.make("memory.create", {
    payload: MemoryCreatePayload,
    success: MemoryEntry,
    error: WireError,
  }),
  Rpc.make("memory.update", {
    payload: MemoryUpdatePayload,
    success: MemoryEntry,
    error: WireError,
  }),
  Rpc.make("memory.delete", {
    payload: MemoryDeletePayload,
    success: DeleteResult,
    error: WireError,
  }),
  // Watch uses a poll token (not a streaming Rpc): the inspector's AtomRpc client
  // exposes streaming Rpcs as pull-based atoms, which don't map cleanly onto a
  // push that must invalidate *other* atoms. A monotonic per-scope version the
  // client polls on a short interval is simpler and robust over the existing
  // same-origin NDJSON transport. The core watcher (real fs events, debounced)
  // is the source of truth; this just exposes its current versions.
  Rpc.make("watch.poll", {
    success: WatchVersions,
  })
);

/** The RPC group type, for client/handler derivation. */
export type PeektraceRpcs = typeof PeektraceRpcs;
