/**
 * The stats wire contract.
 *
 * Two artifacts share this file. `StatsFact` is one line of a cache shard: a
 * flattened, agent-agnostic fact row, already redacted before it touches disk.
 * `StatsReport` is what `peektrace stats --json` prints and what other tools
 * (the harness-doctor skill first) decode. Both carry `schemaVersion`, because
 * an agent reads this payload and acts on it, so a silent shape change silently
 * changes agent behaviour.
 *
 * All TS types are derived with `typeof X.Type` — never hand-duplicated. Absent
 * values are `null`, never omitted: a fact table has fixed columns.
 */
import { Schema } from "effect";
import { AgentId } from "../agent-id";

/** Wire version of the report envelope. Bump on any breaking shape change. */
export const STATS_SCHEMA_VERSION = "peektrace-stats/v2";

/** Bumped when a detector's rule changes, so a row's meaning is versioned. */
export const DETECTOR_VERSION = 2;

/**
 * Bumped when `extract.ts` or any fact column changes. Part of the cache key:
 * a shard written by an older extractor is re-extracted, never read back.
 */
export const EXTRACTOR_VERSION = 6;

/** Where a transcript sits in the session tree. */
export const TranscriptKind = Schema.Literal(
  "main",
  "subagent",
  "workflow-agent"
);
export type TranscriptKind = typeof TranscriptKind.Type;

/** Intent group a command line is charged to. Exclusive, first match wins. */
export const Job = Schema.Literal(
  "wait/poll",
  "test",
  "build",
  "typecheck",
  "lint",
  "package-install",
  "net",
  "script-run",
  "git",
  "search",
  "file-io",
  "other"
);
export type Job = typeof Job.Type;

/** Job ids in priority order. Rank 1 is waiting, which makes the wait row of
 * the time table and the waiting panel the same set by construction. */
export const JOBS = [
  "wait/poll",
  "test",
  "build",
  "typecheck",
  "lint",
  "package-install",
  "net",
  "script-run",
  "git",
  "search",
  "file-io",
  "other",
] as const satisfies readonly Job[];

/** How a command parks: on the clock, or on a condition it re-checks. */
export const WaitShape = Schema.Literal(
  "loop-poll",
  "watcher",
  "curl-retry",
  "bare-delay"
);
export type WaitShape = typeof WaitShape.Type;

/**
 * A named cause of a failed call: one entry of the cause catalog. Detected at
 * extract time while the result text is in hand. `unclassified` never sits on
 * a fact; it names the cause table row for errored calls no detector claimed.
 */
export const CauseId = Schema.Literal(
  "zsh-glob-flag",
  "zsh-glob-path",
  "zsh-equals-abort",
  "pkill-self-kill",
  "sleep-blocked",
  "command-not-found",
  "output-too-large",
  "read-before-write",
  "stale-read",
  "edit-no-match",
  "structured-output-schema",
  "rm-guard",
  "scratch-module-resolution",
  "ugrep-complexity",
  "unclassified"
);
export type CauseId = typeof CauseId.Type;

/**
 * Where a fix lives: a memory-file line (`rule`), the host (`env`), the
 * workflow or subagent prompt (`workflow`), or nowhere anyone measured (`none`).
 */
export const CauseKind = Schema.Literal("rule", "env", "workflow", "none");
export type CauseKind = typeof CauseKind.Type;

/** A count split by who made the call. `nested` is subagent + workflow agent. */
export const Split = Schema.Struct({
  main: Schema.Number,
  nested: Schema.Number,
});
export type Split = typeof Split.Type;

/** A pointer from an aggregate row back to the session it came from. */
export const SampleRef = Schema.Struct({
  sid: Schema.String,
  agent: AgentId,
  project: Schema.String,
  kind: TranscriptKind,
  /** Session a nested transcript runs under; open `sid` through it. */
  parent: Schema.NullOr(Schema.String),
  /** Monotonic index of the fact inside its session; pairs with `sid` as a key. */
  seq: Schema.Number,
  /** The transcript's own call id, so a link lands on the card a UI drew. */
  toolUseId: Schema.NullOr(Schema.String),
  /** One redacted, collapsed, clipped line of evidence. */
  line: Schema.String,
});
export type SampleRef = typeof SampleRef.Type;

/** Columns every fact row carries, whatever its shape. */
const factFields = {
  sid: Schema.String,
  agent: AgentId,
  project: Schema.String,
  kind: TranscriptKind,
  /** Parent session id for nested transcripts, else null. */
  parent: Schema.NullOr(Schema.String),
  /** Workflow run id (`wf_*`) for workflow agents, else null. */
  run: Schema.NullOr(Schema.String),
};

/** One transcript, summarised. Emitted once per file. */
export const SessionFact = Schema.Struct({
  row: Schema.Literal("session"),
  ...factFields,
  startedAt: Schema.NullOr(Schema.String),
  endedAt: Schema.NullOr(Schema.String),
  /** Redacted session title, when the transcript records one. */
  title: Schema.NullOr(Schema.String),
  /** Redacted working directory, so a project's own memory files can be read. */
  cwd: Schema.NullOr(Schema.String),
  /** The agent source the transcript is stored in; null for the default one. */
  source: Schema.NullOr(Schema.String),
  models: Schema.Array(Schema.String),
  turns: Schema.Number,
  userMessages: Schema.Number,
  toolCalls: Schema.Number,
  inputTokens: Schema.Number,
  outputTokens: Schema.Number,
  cacheReadTokens: Schema.Number,
  cacheCreationTokens: Schema.Number,
  /** Largest single-request context across the session's turns. */
  maxContextTokens: Schema.Number,
  /** Cache creation on the first model call: the fan-out prefix cost. */
  firstTurnCacheCreation: Schema.Number,
});
export type SessionFact = typeof SessionFact.Type;

/** One tool call joined to its result. Agent-agnostic. */
export const ToolFact = Schema.Struct({
  row: Schema.Literal("tool"),
  ...factFields,
  /** Monotonic per session, assigned at emit time. Never derived from `ts`. */
  seq: Schema.Number,
  /** The transcript's own call id, so a marker lands on the same card a UI drew. */
  toolUseId: Schema.NullOr(Schema.String),
  tool: Schema.String,
  /** ISO call time, or null when the transcript records none. */
  ts: Schema.NullOr(Schema.String),
  durSec: Schema.NullOr(Schema.Number),
  /** The agent's own error flag on the result. */
  err: Schema.Boolean,
  /** `err`, or the output scanner fired on an exit-0 result. */
  failed: Schema.Boolean,
  /** Why the scanner fired: "tsc" | "lint" | "test" | "traceback" | "node" | ... */
  failReason: Schema.NullOr(Schema.String),
  /** Id of the named detector that marked this row, e.g. "zsh-equals-abort". */
  detector: Schema.NullOr(Schema.String),
  /** Id of the benign filter that suppresses this row from the fail table. */
  benign: Schema.NullOr(Schema.String),
  /** Masked, redacted error signature; the cluster key. */
  sig: Schema.NullOr(Schema.String),
  /** Redacted primary argument: file path, pattern, url, query, subagent type. */
  arg: Schema.NullOr(Schema.String),
  outChars: Schema.Number,
  /** The call sat on a sidechain turn inside its own transcript. */
  side: Schema.Boolean,
  /** Request id of the model call that made this call; parallel siblings share it. */
  req: Schema.NullOr(Schema.String),
  /** Context tokens (input + cache read + cache create) of that model call. */
  ctxTok: Schema.NullOr(Schema.Number),
  /** Cause catalog hit, detected at extract time. */
  cause: Schema.NullOr(CauseId),
  /** The culprit the cause names: glob word, binary, schema field, file. Redacted. */
  causeArg: Schema.NullOr(Schema.String),
  /** The failing shell segment(s), normalised, when the error names the culprit. */
  focus: Schema.NullOr(Schema.String),
});
export type ToolFact = typeof ToolFact.Type;

/** A bash call: everything in `ToolFact` plus the shell parse. */
export const BashFact = Schema.Struct({
  ...ToolFact.fields,
  row: Schema.Literal("bash"),
  /** Redacted command text, clipped. */
  cmd: Schema.String,
  bin: Schema.String,
  /** Normalised command family, e.g. `bun run typecheck`. */
  family: Schema.String,
  job: Job,
  /** Every job present in the line, including the one it was charged to. */
  present: Schema.Array(Job),
  /** Non-null when the line parks instead of doing work. */
  wait: Schema.NullOr(WaitShape),
  /** Literal seconds the line asked to sleep for. Never summed into a total. */
  declaredDelaySec: Schema.Number,
  /** The exit code is discarded by a pipe. */
  piped: Schema.Boolean,
  bg: Schema.Boolean,
  /** Returned at a harness ceiling, so `durSec` is a floor. */
  capped: Schema.Boolean,
});
export type BashFact = typeof BashFact.Type;

/** One line of a cache shard. `row` is the discriminator. */
export const StatsFact = Schema.Union(SessionFact, ToolFact, BashFact);
export type StatsFact = typeof StatsFact.Type;

/** Facts that carry a `seq` and a duration: everything but the session row. */
export type CallFact = ToolFact | BashFact;

/** True for a bash row. The one narrowing every detector needs. */
export const isBashFact = (fact: StatsFact): fact is BashFact =>
  fact.row === "bash";

/** True for a tool-or-bash row, i.e. anything with `seq` and `durSec`. */
export const isCallFact = (fact: StatsFact): fact is CallFact =>
  fact.row !== "session";

/** One row of the fail table. */
export const FailRow = Schema.Struct({
  /** Stable: the masked signature or the scanner reason. Drill key. */
  key: Schema.String,
  label: Schema.String,
  /** "Bash exit 0" | "StructuredOutput" | ... */
  where: Schema.String,
  rows: Schema.Number,
  sessions: Schema.Number,
  projects: Schema.Number,
  /** Of `rows`, how many set the agent's error flag. */
  flagged: Schema.Number,
  /** One line from a named detector. */
  note: Schema.NullOr(Schema.String),
  /** Detector id, versioned with the report. */
  detector: Schema.NullOr(Schema.String),
  samples: Schema.Array(SampleRef),
});
export type FailRow = typeof FailRow.Type;

/** One row of the time table. */
export const TimeRow = Schema.Struct({
  key: Job,
  label: Schema.String,
  /** Charged exclusively: one line, one job. */
  runs: Schema.Number,
  /** Lines holding this job but charged to a higher-priority one. */
  coPresentRuns: Schema.Number,
  totalSec: Schema.Number,
  /** Of `corpus.bashSec`. */
  share: Schema.Number,
  medianSec: Schema.Number,
  p95Sec: Schema.Number,
  /** Returned at a harness ceiling: >= 590s, or 118-124s. */
  cappedRuns: Schema.Number,
  cappedSec: Schema.Number,
  /** Backgrounded: `durSec` understates the wait. */
  bgRuns: Schema.Number,
  bgSec: Schema.Number,
  samples: Schema.Array(SampleRef),
});
export type TimeRow = typeof TimeRow.Type;

/** One waiting shape, counted. */
export const WaitShapeRow = Schema.Struct({
  key: WaitShape,
  calls: Schema.Number,
  totalSec: Schema.Number,
  share: Schema.Number,
});
export type WaitShapeRow = typeof WaitShapeRow.Type;

/**
 * The waiting panel. Reads the same rows as the `wait/poll` time row, because
 * waiting is rank 1 of the exclusive priority order.
 */
export const WaitPanel = Schema.Struct({
  calls: Schema.Number,
  totalSec: Schema.Number,
  /** Of `corpus.bashSec`. */
  share: Schema.Number,
  sessions: Schema.Number,
  projects: Schema.Number,
  medianSec: Schema.Number,
  p95Sec: Schema.Number,
  byShape: Schema.Array(WaitShapeRow),
  /** Backgrounded waits: charged seconds, kept in the set. */
  bgRuns: Schema.Number,
  bgSec: Schema.Number,
  /** Seconds the backgrounded rows declared. Printed beside, never summed in. */
  bgDeclaredSec: Schema.Number,
  samples: Schema.Array(SampleRef),
});
export type WaitPanel = typeof WaitPanel.Type;

/** Errors sharing a masked signature. */
export const ClusterRow = Schema.Struct({
  /** The masked signature. Drill key. */
  key: Schema.String,
  label: Schema.String,
  tool: Schema.String,
  rows: Schema.Number,
  sessions: Schema.Number,
  projects: Schema.Number,
  /** Wall clock charged to the rows in this cluster. */
  totalSec: Schema.Number,
  samples: Schema.Array(SampleRef),
});
export type ClusterRow = typeof ClusterRow.Type;

/** A session ranked by the context it accumulated. */
export const ContextRow = Schema.Struct({
  key: Schema.String,
  sid: Schema.String,
  agent: AgentId,
  project: Schema.String,
  kind: TranscriptKind,
  maxContextTokens: Schema.Number,
  cacheReadTokens: Schema.Number,
  cacheCreationTokens: Schema.Number,
  outputTokens: Schema.Number,
});
export type ContextRow = typeof ContextRow.Type;

/** One workflow run, costed by the prefix every spawned agent re-pays. */
export const FanoutRow = Schema.Struct({
  /** The `wf_*` run id. Drill key. */
  key: Schema.String,
  project: Schema.String,
  agents: Schema.Number,
  /** Summed first-turn cache creation across the run's agents. */
  prefixTokens: Schema.Number,
  toolCalls: Schema.Number,
  totalSec: Schema.Number,
});
export type FanoutRow = typeof FanoutRow.Type;

/** The same url or query fetched more than once. */
export const WebRepeatRow = Schema.Struct({
  /** Redacted url or query. Drill key. */
  key: Schema.String,
  tool: Schema.String,
  calls: Schema.Number,
  sessions: Schema.Number,
  chars: Schema.Number,
  /** True when every repeat happened inside one workflow run. */
  sameRun: Schema.Boolean,
});
export type WebRepeatRow = typeof WebRepeatRow.Type;

/** Token cost rolled up by one dimension. */
export const CostRow = Schema.Struct({
  /** `kind:subagent` or `project:<slug>`. Drill key. */
  key: Schema.String,
  label: Schema.String,
  sessions: Schema.Number,
  inputTokens: Schema.Number,
  outputTokens: Schema.Number,
  cacheReadTokens: Schema.Number,
  cacheCreationTokens: Schema.Number,
});
export type CostRow = typeof CostRow.Type;

/** One memory-file line that already states a cause's rule. */
export const RuleMatch = Schema.Struct({
  /** `~`-collapsed display path; `link → target` when the file is a symlink. */
  path: Schema.String,
  line: Schema.Number,
  /** The matching line, redacted and clipped. */
  excerpt: Schema.String,
});
export type RuleMatch = typeof RuleMatch.Type;

/** A rule that is present in memory and whether the cause still fires since. */
export const RuleCheck = Schema.Struct({
  matches: Schema.Array(RuleMatch),
  /**
   * When the rule appeared: the newest matched file's oldest matched line,
   * first seen by an earlier report or else dated by its file's mtime. Null
   * when unknown.
   */
  sinceIso: Schema.NullOr(Schema.String),
  /** Hits on whole days strictly after that day. */
  hitsSince: Split,
  /** Denominator calls on the same days. */
  callsSince: Split,
  per1kSince: Split,
  /** The env or prompt-level fix: never "add another memory line". */
  advice: Schema.String,
});
export type RuleCheck = typeof RuleCheck.Type;

/** The token change a retry made, e.g. `--include=*.ts` → `--include='*.ts'`. */
export const CauseDiff = Schema.Struct({
  from: Schema.String,
  to: Schema.String,
  pairs: Schema.Number,
});
export type CauseDiff = typeof CauseDiff.Type;

/** One culprit value and how often it was named: a binary, a glob, a field. */
export const CauseDetail = Schema.Struct({
  value: Schema.String,
  hits: Schema.Number,
});
export type CauseDetail = typeof CauseDetail.Type;

/** A failed call and the similar call that succeeded right after it. */
export const PairSample = Schema.Struct({
  failed: SampleRef,
  retry: SampleRef,
});
export type PairSample = typeof PairSample.Type;

/**
 * One cause: what fired, how often the agent paid a round trip to correct it,
 * and the one-line fix. Counts and seconds and tokens stay separate columns:
 * never blended into a score.
 */
export const CauseRow = Schema.Struct({
  /** `cause:<id>`; unclassified rows are `cause:unclassified:<tool>`. Drill key. */
  key: Schema.String,
  cause: CauseId,
  kind: CauseKind,
  label: Schema.String,
  /** Null only when `kind` is "none": a UI prints "no rule to suggest". */
  fix: Schema.NullOr(Schema.String),
  /** False for a cause that is waste rather than failure (output too large). */
  failing: Schema.Boolean,
  hits: Schema.Number,
  /** Of `hits`, how many set the agent's own error flag; the rest exited 0. */
  flagged: Schema.Number,
  hitsBy: Split,
  /**
   * Failed call followed by a similar succeeding call of the same tool: one
   * wasted round trip each. For persisted output, a read-back of the file.
   */
  pairs: Schema.Number,
  pairsBy: Split,
  retrySessions: Schema.Number,
  sessions: Schema.Number,
  projects: Schema.Number,
  /** Call-to-call gap summed over pairs whose gap is known. */
  retrySec: Schema.Number,
  secMeasured: Schema.Number,
  /** Context re-read by the retry turns, deduped per model call. */
  retryCtxTokens: Schema.Number,
  tokensMeasured: Schema.Number,
  /** Hits per 1,000 denominator calls: shell calls for a shell cause, else
   * calls of the tools it watches. */
  per1k: Split,
  details: Schema.Array(CauseDetail),
  diffs: Schema.Array(CauseDiff),
  rule: Schema.NullOr(RuleCheck),
  samples: Schema.Array(SampleRef),
  pairSamples: Schema.Array(PairSample),
});
export type CauseRow = typeof CauseRow.Type;

/** Corpus totals. Every share in the report divides by a field here. */
export const CorpusSummary = Schema.Struct({
  transcripts: Schema.Number,
  sessions: Schema.Number,
  projects: Schema.Number,
  toolCalls: Schema.Number,
  bashCalls: Schema.Number,
  /** Seconds charged to bash calls: the denominator of every time share. */
  bashSec: Schema.Number,
  /** Seconds charged to every tool, bash included. */
  toolSec: Schema.Number,
  /** Seconds charged to tools that wait on the human. Excluded from `toolSec`. */
  humanWaitSec: Schema.Number,
  /** Transcripts per kind, keys sorted. */
  kinds: Schema.Record({ key: Schema.String, value: Schema.Number }),
  /** Transcripts per agent, keys sorted. */
  agents: Schema.Record({ key: Schema.String, value: Schema.Number }),
});
export type CorpusSummary = typeof CorpusSummary.Type;

/** The window the report covers, resolved from an injected clock. */
export const StatsWindow = Schema.Struct({
  days: Schema.Number,
  fromIso: Schema.String,
  toIso: Schema.String,
});
export type StatsWindow = typeof StatsWindow.Type;

/** The versioned artifact. Byte-identical across two runs of unchanged data. */
export const StatsReport = Schema.Struct({
  schemaVersion: Schema.Literal(STATS_SCHEMA_VERSION),
  /** Epoch ms from the injected clock, never `Date.now()` inside the walk. */
  generatedAt: Schema.Number,
  window: StatsWindow,
  detectorVersion: Schema.Number,
  corpus: CorpusSummary,
  /** Named causes with their retry cost. Findings lead with these. */
  causes: Schema.Array(CauseRow),
  fails: Schema.Array(FailRow),
  time: Schema.Array(TimeRow),
  waiting: WaitPanel,
  clusters: Schema.Array(ClusterRow),
  context: Schema.Array(ContextRow),
  fanout: Schema.Array(FanoutRow),
  web: Schema.Array(WebRepeatRow),
  cost: Schema.Array(CostRow),
  /** Unreadable or skipped files, named. */
  skipped: Schema.Array(Schema.String),
  /** Travels with the numbers, never dropped by a UI. */
  caveats: Schema.Array(Schema.String),
});
export type StatsReport = typeof StatsReport.Type;

/** Which table a drill key belongs to. */
export const DrillTable = Schema.Literal(
  "cause",
  "fail",
  "time",
  "cluster",
  "wait"
);
export type DrillTable = typeof DrillTable.Type;

/** One call behind an aggregate row. */
export const DrillHit = Schema.Struct({
  ...SampleRef.fields,
  ts: Schema.NullOr(Schema.String),
  durSec: Schema.NullOr(Schema.Number),
  tool: Schema.String,
  failed: Schema.Boolean,
  /** For a cause drill: the tool-use id of the retry that corrected this
   * call, so its link lands on its card. */
  pairToolUseId: Schema.NullOr(Schema.String),
  /** That retry's own redacted line, null when no retry corrected this call;
   * `line` stays the failed call's. */
  pairLine: Schema.NullOr(Schema.String),
});
export type DrillHit = typeof DrillHit.Type;

/** The rows behind one aggregate key, one page at a time. */
export const DrillResult = Schema.Struct({
  schemaVersion: Schema.Literal(STATS_SCHEMA_VERSION),
  table: DrillTable,
  key: Schema.String,
  /** Matching rows in the corpus, before the page cut. */
  total: Schema.Number,
  /** Index of the first hit in `hits`, counted from the whole match set. */
  offset: Schema.Number,
  hits: Schema.Array(DrillHit),
});
export type DrillResult = typeof DrillResult.Type;

/** One marker on a tool call, keyed so the transcript and the tables agree. */
export const SessionMarker = Schema.Struct({
  seq: Schema.Number,
  /** Tool-use id when the transcript records one, so a UI can key on it. */
  toolUseId: Schema.NullOr(Schema.String),
  detector: Schema.String,
  label: Schema.String,
});
export type SessionMarker = typeof SessionMarker.Type;

/** Every marker for one session. */
export const SessionMarkers = Schema.Struct({
  schemaVersion: Schema.Literal(STATS_SCHEMA_VERSION),
  sid: Schema.String,
  detectorVersion: Schema.Number,
  markers: Schema.Array(SessionMarker),
});
export type SessionMarkers = typeof SessionMarkers.Type;

/** What a `refresh` pass did. */
export const RefreshSummary = Schema.Struct({
  transcripts: Schema.Number,
  extracted: Schema.Number,
  cached: Schema.Number,
  skipped: Schema.Array(Schema.String),
  facts: Schema.Number,
});
export type RefreshSummary = typeof RefreshSummary.Type;
