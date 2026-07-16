import { Schema } from "effect";
import { AgentId } from "../agent-id";

/** Which coding agent produced a transcript. Mirrors the parseable `AgentId`s. */
export const Provider = Schema.Literal(
  "claude-code",
  "codex",
  "pi",
  "opencode"
);
export type Provider = typeof Provider.Type;

/**
 * Schemas for Claude transcript ingest and the serializable analysis result.
 * Input line schemas describe the raw JSONL shape (used for typing + tolerant
 * decode); output schemas describe everything that leaves core over RPC. All
 * TS types are derived from these schemas — never hand-duplicated.
 */

/** Ground-truth token usage carried on assistant lines. */
export const Usage = Schema.Struct({
  input_tokens: Schema.optional(Schema.Number),
  output_tokens: Schema.optional(Schema.Number),
  cache_read_input_tokens: Schema.optional(Schema.Number),
  cache_creation_input_tokens: Schema.optional(Schema.Number),
});
export type Usage = typeof Usage.Type;

/** Fields shared by every transcript line, regardless of `type`. */
const commonFields = {
  cwd: Schema.optional(Schema.String),
  gitBranch: Schema.optional(Schema.String),
  parentUuid: Schema.optional(Schema.NullOr(Schema.String)),
  isSidechain: Schema.optional(Schema.Boolean),
  requestId: Schema.optional(Schema.String),
  timestamp: Schema.optional(Schema.String),
  version: Schema.optional(Schema.String),
  uuid: Schema.optional(Schema.String),
};

/** A user line: prompt text or an array of content blocks (incl. tool results). */
export const UserLine = Schema.Struct({
  type: Schema.Literal("user"),
  message: Schema.optional(Schema.Unknown),
  isCompactSummary: Schema.optional(Schema.Boolean),
  isMeta: Schema.optional(Schema.Boolean),
  ...commonFields,
});

/** An assistant line: carries model, usage, and content blocks. */
export const AssistantLine = Schema.Struct({
  type: Schema.Literal("assistant"),
  message: Schema.optional(Schema.Unknown),
  ...commonFields,
});

/** A system event line. */
export const SystemLine = Schema.Struct({
  type: Schema.Literal("system"),
  content: Schema.optional(Schema.Unknown),
  subtype: Schema.optional(Schema.String),
  ...commonFields,
});

/** An attachment line: injects context (files, listings, reminders, ...). */
export const AttachmentLine = Schema.Struct({
  type: Schema.Literal("attachment"),
  attachment: Schema.optional(Schema.Unknown),
  ...commonFields,
});

/** An ai-title line carrying the model-assigned session title. */
export const AiTitleLine = Schema.Struct({
  type: Schema.Literal("ai-title"),
  aiTitle: Schema.optional(Schema.String),
  ...commonFields,
});

/** Control-metadata line types intentionally dropped during timeline build. */
export const ControlLine = Schema.Struct({
  type: Schema.Literal(
    "mode",
    "permission-mode",
    "file-history-snapshot",
    "last-prompt"
  ),
  ...commonFields,
});

/** Discriminated union over the `type` field of a Claude transcript line. */
export const TranscriptLine = Schema.Union(
  UserLine,
  AssistantLine,
  SystemLine,
  AttachmentLine,
  AiTitleLine,
  ControlLine
);
export type TranscriptLine = typeof TranscriptLine.Type;

/** High-level classification of a single timeline event. */
export const EventKind = Schema.Literal(
  "user-prompt",
  "assistant-text",
  "assistant-thinking",
  "tool-call",
  "tool-result",
  "attachment",
  "system",
  "compaction",
  "summary",
  "meta"
);
export type EventKind = typeof EventKind.Type;

/** Budget category an attachment contributes to (null = not persistent context). */
export const LoadedCategory = Schema.Literal(
  "claude-md",
  "skills",
  "agents",
  "tools",
  "mcp",
  "file",
  "memory",
  "ide",
  "reminder",
  "other"
);
export type LoadedCategory = typeof LoadedCategory.Type;

/** One ordered entry in the reconstructed session timeline. */
export const TimelineEvent = Schema.Struct({
  index: Schema.Number,
  kind: EventKind,
  ts: Schema.optional(Schema.String),
  requestId: Schema.optional(Schema.String),
  isSidechain: Schema.optional(Schema.Boolean),
  title: Schema.String,
  preview: Schema.String,
  body: Schema.String,
  tokensEst: Schema.Number,
  isError: Schema.optional(Schema.Boolean),
  toolName: Schema.optional(Schema.String),
  toolUseId: Schema.optional(Schema.String),
  attachmentType: Schema.optional(Schema.String),
  loadedCategory: Schema.optional(LoadedCategory),
});
export type TimelineEvent = typeof TimelineEvent.Type;

/** One model call (grouped by requestId), carrying ground-truth usage. */
export const Turn = Schema.Struct({
  requestId: Schema.String,
  ts: Schema.optional(Schema.String),
  model: Schema.String,
  contextTokens: Schema.Number,
  inputTokens: Schema.Number,
  cacheReadTokens: Schema.Number,
  cacheCreationTokens: Schema.Number,
  outputTokens: Schema.Number,
  eventIndexes: Schema.Array(Schema.Number),
});
export type Turn = typeof Turn.Type;

/** A spawned subagent transcript discovered on disk. */
export const SubagentRef = Schema.Struct({
  id: Schema.String,
  agentType: Schema.optional(Schema.String),
  description: Schema.optional(Schema.String),
  toolUseId: Schema.optional(Schema.String),
  path: Schema.String,
  turns: Schema.Number,
  peakContextTokens: Schema.Number,
});
export type SubagentRef = typeof SubagentRef.Type;

/** A CLAUDE.md / AGENTS.md / memory file read from disk for residual attribution. */
export const OnDiskContextFile = Schema.Struct({
  label: Schema.String,
  path: Schema.String,
  bytes: Schema.Number,
  tokensEst: Schema.Number,
  scope: Schema.Literal("global", "project", "memory"),
});
export type OnDiskContextFile = typeof OnDiskContextFile.Type;

/** Fixed set of context-budget category keys (stacking order, floor -> top). */
export const BudgetKey = Schema.Literal(
  "system_tools",
  "listings",
  "memory",
  "files",
  "prompts",
  "tool_results",
  "assistant_text",
  "thinking",
  "other",
  "unattributed"
);
export type BudgetKey = typeof BudgetKey.Type;

/** Per-category token map; keys always sum (with unattributed) to a turn's ctx. */
export const BudgetSlices = Schema.Struct({
  system_tools: Schema.Number,
  listings: Schema.Number,
  memory: Schema.Number,
  files: Schema.Number,
  prompts: Schema.Number,
  tool_results: Schema.Number,
  assistant_text: Schema.Number,
  thinking: Schema.Number,
  other: Schema.Number,
  unattributed: Schema.Number,
});
export type BudgetSlices = typeof BudgetSlices.Type;

/** One slice of the context-budget breakdown. */
export const BudgetSlice = Schema.Struct({
  key: BudgetKey,
  label: Schema.String,
  short: Schema.String,
  tokens: Schema.Number,
  color: Schema.String,
  estimated: Schema.Boolean,
  note: Schema.optional(Schema.String),
});
export type BudgetSlice = typeof BudgetSlice.Type;

/** Per-turn category attribution for the stacked-area timeline. */
export const TurnSnapshot = Schema.Struct({
  turnIndex: Schema.Number,
  ts: Schema.optional(Schema.String),
  model: Schema.String,
  ctx: Schema.Number,
  outputTokens: Schema.Number,
  cacheReadTokens: Schema.Number,
  slices: BudgetSlices,
});
export type TurnSnapshot = typeof TurnSnapshot.Type;

/** Parsed-but-not-yet-analyzed session. */
export const ParsedSession = Schema.Struct({
  provider: Provider,
  sessionId: Schema.String,
  path: Schema.String,
  cwd: Schema.optional(Schema.String),
  gitBranch: Schema.optional(Schema.String),
  title: Schema.optional(Schema.String),
  version: Schema.optional(Schema.String),
  models: Schema.Array(Schema.String),
  startedAt: Schema.optional(Schema.String),
  endedAt: Schema.optional(Schema.String),
  events: Schema.Array(TimelineEvent),
  turns: Schema.Array(Turn),
  compactionIndexes: Schema.Array(Schema.Number),
  subagents: Schema.Array(SubagentRef),
  /**
   * Authoritative context-window size read straight from the transcript
   * (Codex `model_context_window`); `analyze` prefers it over the inferred
   * default and flips `contextWindowInferred` off. Absent for agents whose
   * transcripts don't record it (Claude, Pi).
   */
  nativeContextWindow: Schema.optional(Schema.Number),
});
export type ParsedSession = typeof ParsedSession.Type;

/** Fully analyzed session, ready to render (serializable, no HTML). */
export const AnalyzedSession = Schema.Struct({
  ...ParsedSession.fields,
  contextWindow: Schema.Number,
  contextWindowInferred: Schema.Boolean,
  peakContextTokens: Schema.Number,
  peakTurnIndex: Schema.Number,
  finalContextTokens: Schema.Number,
  totalOutputTokens: Schema.Number,
  systemOverheadTokens: Schema.Number,
  durationMs: Schema.optional(Schema.Number),
  budget: Schema.Array(BudgetSlice),
  snapshots: Schema.Array(TurnSnapshot),
  onDiskContextFiles: Schema.Array(OnDiskContextFile),
  dumbZoneCrossTurn: Schema.Number,
  dumbZoneFraction: Schema.Number,
  dumbZoneTurns: Schema.Number,
  compactionTurns: Schema.Array(Schema.Number),
  biggestItems: Schema.Array(TimelineEvent),
  turnCount: Schema.Number,
  userMessageCount: Schema.Number,
  toolCallCount: Schema.Number,
  peakCacheReadTokens: Schema.Number,
});
export type AnalyzedSession = typeof AnalyzedSession.Type;

/** Lightweight session header returned by `list` without a full body parse. */
export const SessionHeader = Schema.Struct({
  id: Schema.String,
  path: Schema.String,
  /** The coding agent that produced this transcript (Claude / Codex / Pi). */
  agent: AgentId,
  cwd: Schema.optional(Schema.String),
  project: Schema.String,
  gitBranch: Schema.optional(Schema.String),
  model: Schema.optional(Schema.String),
  title: Schema.optional(Schema.String),
  startedAt: Schema.optional(Schema.String),
  updatedAt: Schema.optional(Schema.String),
  messageCount: Schema.Number,
  sizeBytes: Schema.Number,
});
export type SessionHeader = typeof SessionHeader.Type;
