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
  "system-prompt",
  "system",
  "compaction",
  "summary",
  "meta"
);
export type EventKind = typeof EventKind.Type;

/**
 * Who wrote a user-side prompt: a person at the keyboard (`human`), a program
 * through the Agent SDK (`sdk`), a slash command and its output (`command`), a
 * background task reporting back (`task-notification`), a wake-up the model
 * scheduled (`scheduled`), or other text the harness wrote (`harness`).
 */
export const PromptOrigin = Schema.Literal(
  "human",
  "sdk",
  "command",
  "task-notification",
  "scheduled",
  "harness"
);
export type PromptOrigin = typeof PromptOrigin.Type;

/**
 * Where an image's bytes sit in the transcript: content block `block` of the
 * event's line, and item `item` inside it when that block is a tool result.
 * Events carry the ref, never the base64, so the payload stays small and the
 * inspector fetches an image only when it is opened.
 */
export const ImageRef = Schema.Struct({
  block: Schema.Number,
  item: Schema.optional(Schema.Number),
  mediaType: Schema.String,
});
export type ImageRef = typeof ImageRef.Type;

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

/**
 * One earlier edit of an entry edited more than once: from line `atIndex` the
 * model saw `tokensEst` visible and `thinkingEst` thinking tokens. The first
 * event of the entry carries the entry's sizes; its other events carry zeros.
 */
export const ContextEditStep = Schema.Struct({
  atIndex: Schema.Number,
  tokensEst: Schema.Number,
  thinkingEst: Schema.Number,
});
export type ContextEditStep = typeof ContextEditStep.Type;

/**
 * A later transcript entry (at line `atIndex`) changed this event's share of the
 * model context. `removed`: dropped from context from then on. `replaced`: the
 * event shows the new content; the first event of the edited entry carries the
 * entry's original text and size. `steps` lists earlier edits of the same entry,
 * oldest first, when the entry was edited more than once.
 */
export const ContextEdit = Schema.Union(
  Schema.Struct({
    status: Schema.Literal("removed"),
    atIndex: Schema.Number,
    steps: Schema.optional(Schema.Array(ContextEditStep)),
  }),
  Schema.Struct({
    status: Schema.Literal("replaced"),
    atIndex: Schema.Number,
    originalTokensEst: Schema.Number,
    original: Schema.optional(Schema.String),
    steps: Schema.optional(Schema.Array(ContextEditStep)),
  })
);
export type ContextEdit = typeof ContextEdit.Type;

/** One ordered entry in the reconstructed session timeline. */
export const TimelineEvent = Schema.Struct({
  index: Schema.Number,
  kind: EventKind,
  ts: Schema.optional(Schema.String),
  requestId: Schema.optional(Schema.String),
  isSidechain: Schema.optional(Schema.Boolean),
  /** The harness wrote this line, not the person at the keyboard. */
  isMeta: Schema.optional(Schema.Boolean),
  /** Who wrote a `user-prompt`; absent when the transcript does not say. */
  promptOrigin: Schema.optional(PromptOrigin),
  /** Claude Code's own `promptSource` for the line (`typed`, `sdk`, `system`…). */
  promptSource: Schema.optional(Schema.String),
  title: Schema.String,
  preview: Schema.String,
  body: Schema.String,
  tokensEst: Schema.Number,
  /**
   * Real size from usage data: the growth in prompt size between the model
   * calls around this event. Absent where only the chars/4 estimate exists.
   */
  tokensMeasured: Schema.optional(Schema.Number),
  isError: Schema.optional(Schema.Boolean),
  /** A server tool's call or result (web search, web fetch, advisor): the API
   *  ran it and wrote the result into the assistant message. */
  serverTool: Schema.optional(Schema.Boolean),
  /** Wall time the model spent on this thinking block, when the harness logs it. */
  thinkingDurationMs: Schema.optional(Schema.Number),
  toolName: Schema.optional(Schema.String),
  toolUseId: Schema.optional(Schema.String),
  attachmentType: Schema.optional(Schema.String),
  /** Images on this event's line, fetched on demand (`sessions.image`). */
  images: Schema.optional(Schema.Array(ImageRef)),
  loadedCategory: Schema.optional(LoadedCategory),
  contextEdit: Schema.optional(ContextEdit),
  /** Context size before a compaction, from Claude's `compact_boundary`. */
  compactPreTokens: Schema.optional(Schema.Number),
  /** Context size right after a compaction, from Claude's `compact_boundary`. */
  compactPostTokens: Schema.optional(Schema.Number),
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
  /**
   * Cache writes with a 1-hour TTL (`usage.cache_creation`). With the 5-minute
   * count it sums to `cacheCreationTokens` when reported; absent on old transcripts.
   */
  cacheCreation1hTokens: Schema.optional(Schema.Number),
  /** Cache writes with a 5-minute TTL; see `cacheCreation1hTokens`. */
  cacheCreation5mTokens: Schema.optional(Schema.Number),
  outputTokens: Schema.Number,
  /**
   * The thinking share of `outputTokens`, from `usage.output_tokens_details`.
   * Absent on transcripts that do not report it.
   */
  thinkingTokens: Schema.optional(Schema.Number),
  /** Reasoning effort the harness asked for on this call (`low` … `max`). */
  effort: Schema.optional(Schema.String),
  /** Server-side web searches the API ran in this call (`server_tool_use`). */
  webSearchRequests: Schema.optional(Schema.Number),
  /** Server-side web fetches the API ran in this call (`server_tool_use`). */
  webFetchRequests: Schema.optional(Schema.Number),
  eventIndexes: Schema.Array(Schema.Number),
});
export type Turn = typeof Turn.Type;

/** A spawned subagent transcript discovered on disk. */
export const SubagentRef = Schema.Struct({
  id: Schema.String,
  agentType: Schema.optional(Schema.String),
  description: Schema.optional(Schema.String),
  /**
   * The call that launched this agent: the Workflow call for workflow agents.
   * When `parentAgentId` is set, the call is in the parent subagent's
   * transcript, not the main events.
   */
  toolUseId: Schema.optional(Schema.String),
  path: Schema.String,
  turns: Schema.Number,
  peakContextTokens: Schema.Number,
  /** The workflow run this agent belongs to: the `wf_*` dir name. */
  workflowRunId: Schema.optional(Schema.String),
  /** The run phase title, from meta or the run json. */
  workflowPhase: Schema.optional(Schema.String),
  /** The run phase position, from the run json; sort phases by it. */
  workflowPhaseIndex: Schema.optional(Schema.Number),
  /** The subagent that spawned this one, as `agent-<id>` like stub ids. */
  parentAgentId: Schema.optional(Schema.String),
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
   * default and flips `contextWindowInferred` off. Claude sets it to 1M when a
   * `cost-state` line bills a `[1m]` model id; Pi guesses it from the model id.
   */
  nativeContextWindow: Schema.optional(Schema.Number),
  /** Record types the parser does not know, with line counts. Set only when non-empty. */
  unknownRecordTypes: Schema.optional(
    Schema.Record({ key: Schema.String, value: Schema.Number })
  ),
  /** `system` subtypes the parser does not know, with line counts. Set only when non-empty. */
  unknownSystemSubtypes: Schema.optional(
    Schema.Record({ key: Schema.String, value: Schema.Number })
  ),
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
  turnCount: Schema.Number,
  userMessageCount: Schema.Number,
  toolCallCount: Schema.Number,
  peakCacheReadTokens: Schema.Number,
  /** Server-side web searches across all counted turns. */
  webSearchRequests: Schema.Number,
  /** Server-side web fetches across all counted turns. */
  webFetchRequests: Schema.Number,
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
  /** Stable source id when the owning agent has >1 configured root. */
  source: Schema.optional(Schema.String),
  /** Human label for that source (e.g. "work"), shown as a badge. */
  sourceLabel: Schema.optional(Schema.String),
});
export type SessionHeader = typeof SessionHeader.Type;
