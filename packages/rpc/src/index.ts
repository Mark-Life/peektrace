/** Public surface of `@workspace/rpc`: the Effect-RPC contract, the wired
 * handler layer factory, and the typed clients (HTTP + in-process). */
export {
  createPeepholeClient,
  makeInProcessClient,
  protocolLayer,
} from "./client";
export {
  Capability,
  CapabilitySupport,
  CapabilityUnsupportedError,
  FileChangedError,
  MemoryCreatePayload,
  MemoryDeletePayload,
  MemoryFrontmatterPatch,
  MemoryNotFoundError,
  MemoryUpdatePayload,
  MemoryValidationError,
  MemoryVaultPayload,
  PathOutsideRootError,
  PeepholeRpcs,
  SessionAnalyzePayload,
  SessionGetPayload,
  SessionNotFoundError,
  SessionsListPayload,
  SupportLevel,
  TranscriptParseError,
  WatchVersions,
  WireError,
} from "./contract";
export {
  type HandlersLayerOptions,
  makeHandlersLayer,
} from "./handlers";
