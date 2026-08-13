export {
  AGENT_IDS,
  AgentId,
  AgentRegistry,
  AgentRegistryLive,
  type AgentRegistryShape,
  type AgentRoots,
  type AgentSource,
  AgentUnsupportedError,
  computeRoots,
  type SessionFileRef,
  type SessionLayout,
  settingsPath,
  sourcesOf,
} from "./services/agents";
export {
  type Capability,
  CapabilityRegistry,
  CapabilityRegistryLive,
  type CapabilityRegistryShape,
  type CapabilitySupport,
  type SupportLevel,
  seededCapabilities,
} from "./services/capabilities";
export {
  FileChangedError,
  type FileStat,
  FsLive,
  FsReadOnly,
  PathOutsideRootError,
  ReadFs,
  type ReadFsShape,
  WriteDeniedError,
  type WriteExpectation,
  WriteFs,
  type WriteFsShape,
} from "./services/fs";
export * from "./services/memory";
export {
  type AnalyzeOptions,
  analyze,
  CAT_META,
} from "./services/sessions/analyze";
export {
  SessionNotFoundError,
  TranscriptParseError,
} from "./services/sessions/errors";
export { buildHeader } from "./services/sessions/header";
export {
  type ParseClaudeArgs,
  parseClaudeSession,
  parseJsonl,
} from "./services/sessions/parse";
export {
  PARSERS,
  parseCodexSession,
  parsePiSession,
  type SessionParser,
} from "./services/sessions/parsers";
export {
  redactParsed,
  redactSession,
  redactText,
} from "./services/sessions/redact";
export {
  findSubagents,
  gatherOnDiskContextFiles,
  type ResolvedSession,
  resolveClaudeSession,
  type SubagentStub,
} from "./services/sessions/resolve";
export {
  AiTitleLine,
  AnalyzedSession,
  AssistantLine,
  AttachmentLine,
  BudgetKey,
  BudgetSlice,
  BudgetSlices,
  ControlLine,
  EventKind,
  LoadedCategory,
  OnDiskContextFile,
  ParsedSession,
  Provider,
  SessionHeader,
  SubagentRef,
  SystemLine,
  TimelineEvent,
  TranscriptLine,
  Turn,
  TurnSnapshot,
  Usage,
  UserLine,
} from "./services/sessions/schema";
export {
  type AnalyzeRequest,
  type ParseRequest,
  SessionsService,
  SessionsServiceLive,
  type SessionsServiceShape,
} from "./services/sessions/service";
export {
  loadSettings,
  type PeektraceSettings,
  parseSettings,
} from "./services/settings";
export {
  type SettingsResult,
  SettingsService,
  SettingsServiceLive,
  type SettingsServiceShape,
  SettingsWriteError,
} from "./services/settings-service";
export {
  StatsCache,
  StatsCacheLive,
  type StatsCacheShape,
  statsCacheRoot,
} from "./services/stats/cache";
export {
  StatsCacheError,
  StatsRowNotFoundError,
  StatsScanError,
} from "./services/stats/errors";
export {
  type Finding,
  type FindingFormat,
  failSpread,
  findingsOf,
  timeNote,
  timeSpread,
} from "./services/stats/findings";
export {
  CorpusSummary,
  DETECTOR_VERSION,
  DrillResult,
  DrillTable,
  FailRow,
  RefreshSummary,
  SessionMarker,
  SessionMarkers,
  STATS_SCHEMA_VERSION,
  StatsReport,
  TimeRow,
  WaitPanel,
} from "./services/stats/schema";
export {
  type DrillRequest,
  type MarkersRequest,
  type RefreshRequest,
  type ReportRequest,
  StatsService,
  StatsServiceLive,
  type StatsServiceShape,
} from "./services/stats/service";
export {
  listCorpus,
  listTranscripts,
  type TranscriptRef,
} from "./services/stats/walk";
export {
  type Invalidation,
  type WatchScope,
  WatchService,
  WatchServiceLive,
  type WatchServiceShape,
  type WatchVersionsShape,
} from "./services/watch";
