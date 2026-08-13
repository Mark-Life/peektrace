/** Cross-session friction stats: the wire contract, the walk, and the service. */
export {
  type AccumulatorFactory,
  type AccumulatorOptions,
  addToTally,
  type CorpusTotals,
  type DrillTarget,
  type FactAccumulator,
  type FactSink,
  type FinishContext,
  HUMAN_WAIT_TOOLS,
  hitOf,
  makeCorpusTotals,
  sampleOf,
  samplesOf,
  type Tally,
  tallyFor,
} from "./accumulate";
export {
  type CacheEntry,
  isCacheable,
  isFresh,
  StatsCache,
  StatsCacheLive,
  type StatsCacheShape,
  statsCacheRoot,
} from "./cache";
export {
  StatsCacheError,
  StatsRowNotFoundError,
  StatsScanError,
} from "./errors";
export { extractFacts } from "./extract";
export { type MarkerCollector, makeMarkerCollector } from "./markers";
export {
  clusterKeyOf,
  errorSignature,
  type NormalizedCommand,
  normalizeCommand,
} from "./normalize";
export * from "./rank";
export { defaultRedactor, makeRedactor, type Redactor } from "./redact";
export * from "./schema";
export {
  type DrillRequest,
  type MarkersRequest,
  type RefreshRequest,
  type ReportRequest,
  StatsService,
  StatsServiceLive,
  type StatsServiceShape,
} from "./service";
export { splitSegments, stripHeredocs, tokenize } from "./shell-lex";
export {
  classifyParts,
  foldCorpus,
  listCorpus,
  listTranscripts,
  statOf,
  type TranscriptRef,
  WALK_CONCURRENCY,
} from "./walk";
