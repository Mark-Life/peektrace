/** Parse a Claude Code JSONL transcript into a normalized ParsedSession. */
import { attachmentEvent, type PromptSeen } from "./claude-attachments";
import { deadBranchRows } from "./claude-branches";
import {
  bump,
  type CompactTokens,
  compactMetaOf,
  countsRecord,
  foldClaudeUsage,
  hasOneMillionKey,
  isRealModel,
  KNOWN_IGNORED_TYPES,
  KNOWN_SYSTEM_SUBTYPES,
} from "./claude-records";
import {
  type AssistantBase,
  advisorResultEvent,
  type Block,
  serverResultEvent,
  str,
} from "./claude-server-results";
import { turnsCost } from "./cost";
import {
  imageMarker,
  imageRef,
  isImageBlock,
  omitImageData,
  toolResultImageRefs,
} from "./images";
import { measureTokens } from "./measure";
import { promptEvent } from "./prompt-origin";
import type { ParsedSession, TimelineEvent, Turn } from "./schema";
import { estTokens, firstLine } from "./tokens";
import { opt, serverToolCounts, thinkingTokensOf } from "./usage";

/** A raw JSONL line, untyped. */
type RawLine = Record<string, unknown>;

/** Mutable turn used while building (schema `Turn` has readonly eventIndexes). */
type MutTurn = Omit<Turn, "eventIndexes"> & { eventIndexes: number[] };

const WHITESPACE = /\s+/g;
const ONE_MILLION = 1_000_000;

/**
 * Parse JSONL text into raw lines, skipping blanks and malformed rows.
 * `malformed` counts bad rows except the last non-blank one, which is the
 * partial write of a live session rather than damage.
 */
export const scanJsonl = (text: string) => {
  const rows: RawLine[] = [];
  let bad = 0;
  let lastBad = false;
  for (const line of text.split("\n")) {
    if (!line.trim()) {
      continue;
    }
    try {
      rows.push(JSON.parse(line) as RawLine);
      lastBad = false;
    } catch {
      bad += 1;
      lastBad = true;
    }
  }
  return { rows, malformed: lastBad ? bad - 1 : bad };
};

/** Parse JSONL text into raw lines, skipping blanks and malformed rows. */
export const parseJsonl = (text: string): RawLine[] => scanJsonl(text).rows;

/**
 * Line `n` as `parseJsonl` numbers it, parsing only that line. A line counts
 * when it looks like one JSON object; `parseJsonl` also drops a line that only
 * looks complete, so a caller that cannot confirm the result should fall back
 * to the full parse.
 */
export const jsonlLineAt = (text: string, n: number): unknown => {
  let seen = 0;
  let start = 0;
  while (start <= text.length) {
    const end = text.indexOf("\n", start);
    const line = text.slice(start, end < 0 ? text.length : end).trim();
    if (line.startsWith("{") && line.endsWith("}")) {
      if (seen === n) {
        try {
          return JSON.parse(line) as unknown;
        } catch {
          return;
        }
      }
      seen++;
    }
    if (end < 0) {
      return;
    }
    start = end + 1;
  }
  return;
};

interface ParseState {
  readonly compactionIndexes: number[];
  readonly events: TimelineEvent[];
  readonly models: Set<string>;
  /** A `cost-state` line billed a `[1m]` model id. */
  oneMillion: boolean;
  /** Token counts of the last `compact_boundary`, for the next summary. */
  pendingCompact: CompactTokens | undefined;
  /** Prompt parts already shown, per thread (main or one subagent). */
  readonly promptSeen: Map<string, PromptSeen>;
  /** Inline `isSidechain` turns left out of `turnsById`; priced apart. */
  readonly sidechainById: Map<string, MutTurn>;
  readonly turnsById: Map<string, MutTurn>;
  readonly unknownSubtypes: Map<string, number>;
  readonly unknownTypes: Map<string, number>;
}

/** Mutable session-level metadata gathered while scanning lines. */
interface Meta {
  cwd?: string;
  endedAt?: string;
  gitBranch?: string;
  startedAt?: string;
  title?: string;
  version?: string;
}

interface LineCtx {
  readonly index: number;
  readonly isSidechain: boolean;
  readonly o: RawLine;
  readonly state: ParseState;
  readonly ts: string | undefined;
}

/** Update session metadata from one line's common fields. */
const applyMeta = (meta: Meta, o: RawLine, ts: string | undefined) => {
  if (ts) {
    meta.startedAt ??= ts;
    meta.endedAt = ts;
  }
  if (typeof o.cwd === "string" && !meta.cwd) {
    meta.cwd = o.cwd;
  }
  if (typeof o.gitBranch === "string") {
    meta.gitBranch = o.gitBranch;
  }
  if (typeof o.version === "string") {
    meta.version = o.version;
  }
};

/** Build a tool_result / text event from a user content block. */
const userBlockEvent = (args: {
  readonly block: Record<string, unknown>;
  readonly index: number;
  readonly ts: string | undefined;
  readonly isSidechain: boolean;
  readonly o: RawLine;
  readonly blockIndex: number;
}): TimelineEvent | null => {
  const { block, index, ts, isSidechain, o, blockIndex } = args;
  if (block?.type === "tool_result") {
    const raw = omitImageData(block.content);
    const text = typeof raw === "string" ? raw : str(raw);
    const images = toolResultImageRefs(blockIndex, block.content);
    return {
      index,
      kind: "tool-result",
      isSidechain,
      title: `tool_result${block.is_error ? " (error)" : ""}`,
      preview: firstLine(text),
      body: text,
      tokensEst: estTokens(text),
      isError: block.is_error === true,
      ...opt("ts", ts),
      ...opt("images", images.length > 0 ? images : undefined),
      ...opt(
        "toolUseId",
        typeof block.tool_use_id === "string" ? block.tool_use_id : undefined
      ),
    };
  }
  if (block?.type === "text") {
    return promptEvent({
      o,
      index,
      ts,
      isSidechain,
      text: str(block.text),
      title: "User message",
    });
  }
  if (isImageBlock(block)) {
    const text = imageMarker(block);
    return {
      index,
      kind: "attachment",
      isSidechain,
      title: "image",
      preview: text,
      body: text,
      tokensEst: estTokens(text),
      attachmentType: "image",
      loadedCategory: "file",
      images: [imageRef({ block, blockIndex })],
      ...opt("ts", ts),
    };
  }
  return null;
};

/** Build an assistant content event (thinking / text / tool_use). */
const assistantBlockEvent = (args: {
  readonly block: Block;
  readonly base: AssistantBase;
  readonly thinkingDurationMs: number | undefined;
}): TimelineEvent | null => {
  const { block, base } = args;
  switch (block?.type) {
    case "thinking":
    case "redacted_thinking": {
      const text = str(block.thinking);
      return {
        ...base,
        kind: "assistant-thinking",
        title: "Thinking",
        preview: text ? firstLine(text) : "(content not stored in transcript)",
        body: text,
        tokensEst: estTokens(text),
        ...opt("thinkingDurationMs", args.thinkingDurationMs),
      };
    }
    case "text": {
      const text = str(block.text);
      return {
        ...base,
        kind: "assistant-text",
        title: "Assistant",
        preview: firstLine(text),
        body: text,
        tokensEst: estTokens(text),
      };
    }
    case "tool_use":
    case "server_tool_use": {
      const inputStr = JSON.stringify(block.input ?? {}, null, 2);
      return {
        ...base,
        kind: "tool-call",
        title: String(block.name ?? "tool"),
        preview: firstLine(inputStr.replace(WHITESPACE, " ")),
        body: inputStr,
        tokensEst: estTokens(inputStr),
        toolName: String(block.name ?? "tool"),
        ...opt(
          "toolUseId",
          typeof block.id === "string" ? block.id : undefined
        ),
      };
    }
    case "advisor_tool_result":
      return advisorResultEvent(base, block);
    default:
      return typeof block?.type === "string" &&
        block.type.endsWith("_tool_result")
        ? serverResultEvent(base, block)
        : null;
  }
};

/** Handle one `user` line, appending its events. */
const handleUser = (ctx: LineCtx) => {
  const { o, index, ts, isSidechain, state } = ctx;
  const msg = (o.message ?? {}) as Record<string, unknown>;
  const content = msg.content;
  const isCompact = o.isCompactSummary === true;
  const compactTokens = isCompact ? state.pendingCompact : undefined;
  if (isCompact) {
    state.compactionIndexes.push(index);
    state.pendingCompact = undefined;
  }
  if (typeof content === "string") {
    state.events.push(
      isCompact
        ? {
            index,
            kind: "compaction",
            isSidechain,
            title: "Context compaction (summary)",
            preview: firstLine(content),
            body: content,
            tokensEst: estTokens(content),
            ...opt("ts", ts),
            ...opt("isMeta", o.isMeta === true ? true : undefined),
            ...compactTokens,
          }
        : promptEvent({
            o,
            index,
            ts,
            isSidechain,
            text: content,
            title: "User prompt",
          })
    );
    return;
  }
  if (Array.isArray(content)) {
    for (const [blockIndex, block] of (
      content as Record<string, unknown>[]
    ).entries()) {
      const ev = userBlockEvent({
        block,
        index,
        ts,
        isSidechain,
        o,
        blockIndex,
      });
      if (ev) {
        state.events.push(ev);
      }
    }
  }
};

/** Register the turn for an assistant line, if it should count toward the curve. */
const registerTurn = (args: {
  readonly state: ParseState;
  readonly requestId: string;
  readonly model: string;
  readonly ts: string | undefined;
  readonly usage: Record<string, unknown>;
  readonly countable: boolean;
  readonly effort: string | undefined;
}) => {
  const { state, requestId, model, ts, usage } = args;
  foldClaudeUsage({
    turns: args.countable ? state.turnsById : state.sidechainById,
    key: requestId,
    model,
    usage,
    make: (tokens) => ({
      requestId,
      model,
      ...tokens,
      eventIndexes: [],
      ...opt("ts", ts),
      ...opt("thinkingTokens", thinkingTokensOf(usage)),
      ...serverToolCounts(usage),
      ...opt("effort", args.effort),
    }),
  });
};

/** Handle one `assistant` line: register the turn, append content events. */
const handleAssistant = (
  ctx: LineCtx & { readonly includeSidechainTurns: boolean }
) => {
  const { o, index, ts, isSidechain, state, includeSidechainTurns } = ctx;
  const msg = (o.message ?? {}) as Record<string, unknown>;
  const model = String(msg.model ?? "unknown");
  if (isRealModel(model)) {
    state.models.add(model);
  }
  const requestId =
    typeof o.requestId === "string" ? o.requestId : `line-${index}`;
  registerTurn({
    state,
    requestId,
    model,
    ts,
    usage: (msg.usage ?? {}) as Record<string, unknown>,
    countable: includeSidechainTurns || !isSidechain,
    effort: typeof o.effort === "string" ? o.effort : undefined,
  });
  const turn = state.turnsById.get(requestId);
  for (const block of (msg.content ?? []) as Record<string, unknown>[]) {
    const ev = assistantBlockEvent({
      block,
      base: { index, requestId, isSidechain, ...opt("ts", ts) },
      thinkingDurationMs:
        typeof o.thinkingDurationMs === "number"
          ? o.thinkingDurationMs
          : undefined,
    });
    if (ev) {
      state.events.push(ev);
    }
    if (turn) {
      turn.eventIndexes.push(state.events.length - 1);
    }
  }
};

/** Handle one `summary` line. */
const handleSummary = (ctx: LineCtx) => {
  const { o, index, ts, state } = ctx;
  const text = str(o.summary);
  state.events.push({
    index,
    kind: "summary",
    title: "Rolling summary",
    preview: firstLine(text),
    body: text,
    tokensEst: estTokens(text),
    ...opt("ts", ts),
  });
};

/** The prompt seen-state for the line's thread, created on first use. */
const promptSeenFor = (ctx: LineCtx) => {
  const { o, isSidechain, state } = ctx;
  const agentId = typeof o.agentId === "string" ? o.agentId : "sidechain";
  const thread = isSidechain ? agentId : "main";
  const found = state.promptSeen.get(thread);
  if (found) {
    return found;
  }
  const fresh: PromptSeen = {};
  state.promptSeen.set(thread, fresh);
  return fresh;
};

/** Handle one `attachment` line. */
const handleAttachment = (ctx: LineCtx) => {
  const { o, index, ts, isSidechain, state } = ctx;
  const ev = attachmentEvent({
    o,
    base: { index, ts, isSidechain },
    seen: promptSeenFor(ctx),
  });
  if (ev) {
    state.events.push(ev);
  }
};

/** Handle one `system` line. */
const handleSystem = (ctx: LineCtx) => {
  const { o, index, ts, isSidechain, state } = ctx;
  const text = str(o.content ?? o.subtype ?? "");
  if (o.subtype === "compact_boundary") {
    state.pendingCompact = compactMetaOf(o);
  } else if (
    typeof o.subtype === "string" &&
    !KNOWN_SYSTEM_SUBTYPES.has(o.subtype)
  ) {
    bump(state.unknownSubtypes, o.subtype);
  }
  state.events.push({
    index,
    kind: "system",
    isSidechain,
    title: `system: ${String(o.subtype ?? "event")}`,
    preview: firstLine(text),
    body: text,
    tokensEst: 0,
    ...opt("ts", ts),
  });
};

/**
 * Give thinking blocks the size usage reports for them. A call's
 * `thinking_tokens` goes to its thinking blocks, split by text length (evenly
 * when no text was stored), so a signature-only block still shows its real cost.
 */
const measureThinking = (
  events: readonly TimelineEvent[],
  turns: readonly MutTurn[]
): TimelineEvent[] => {
  const byReq = new Map<string, number[]>();
  events.forEach((e, pos) => {
    if (e.kind === "assistant-thinking" && e.requestId) {
      byReq.set(e.requestId, [...(byReq.get(e.requestId) ?? []), pos]);
    }
  });
  const measured = new Map<number, number>();
  for (const turn of turns) {
    const positions = byReq.get(turn.requestId);
    const reported = turn.thinkingTokens;
    if (reported === undefined || !positions) {
      continue;
    }
    const weights = positions.map((pos) => events[pos]?.tokensEst ?? 0);
    const total = weights.reduce((a, b) => a + b, 0);
    let left = reported;
    positions.forEach((pos, i) => {
      const last = i === positions.length - 1;
      const share =
        total > 0 ? (weights[i] ?? 0) / total : 1 / positions.length;
      const n = last ? left : Math.round(reported * share);
      measured.set(pos, n);
      left -= n;
    });
  }
  return events.map((e, pos) => {
    const tokensMeasured = measured.get(pos);
    return tokensMeasured === undefined ? e : { ...e, tokensMeasured };
  });
};

export interface ParseClaudeArgs {
  /** Count `isSidechain` turns toward the curve (on only for a subagent's own file). */
  readonly includeSidechainTurns?: boolean;
  readonly path: string;
  readonly sessionId: string;
  /** Leave out `prompt_snapshot` system-prompt events, for callers that never read them. */
  readonly skipSystemPrompt?: boolean;
  readonly text: string;
}

/**
 * Parse a Claude Code transcript into a ParsedSession.
 * Token usage is taken verbatim from assistant `usage`; body sizes are chars/4.
 */
export const parseClaudeSession = (args: ParseClaudeArgs): ParsedSession => {
  const {
    text,
    path,
    sessionId,
    includeSidechainTurns = false,
    skipSystemPrompt = false,
  } = args;
  const state: ParseState = {
    events: [],
    turnsById: new Map(),
    sidechainById: new Map(),
    compactionIndexes: [],
    models: new Set(),
    oneMillion: false,
    pendingCompact: undefined,
    promptSeen: new Map(),
    unknownSubtypes: new Map(),
    unknownTypes: new Map(),
  };
  const meta: Meta = {};

  const lines = parseJsonl(text);
  // Rows abandoned by a rewind still feed meta, never turns or events.
  const dead = deadBranchRows(lines);
  lines.forEach((o, index) => {
    const type = typeof o.type === "string" ? o.type : "";
    const ts = typeof o.timestamp === "string" ? o.timestamp : undefined;
    applyMeta(meta, o, ts);
    if (dead.has(index)) {
      return;
    }
    const ctx: LineCtx = {
      o,
      index,
      ts,
      isSidechain: o.isSidechain === true,
      state,
    };
    switch (type) {
      case "ai-title":
        if (typeof o.aiTitle === "string") {
          meta.title = o.aiTitle;
        }
        break;
      case "attachment":
        if (
          !(
            skipSystemPrompt &&
            (o.attachment as RawLine | undefined)?.type === "prompt_snapshot"
          )
        ) {
          handleAttachment(ctx);
        }
        break;
      case "user":
        handleUser(ctx);
        break;
      case "assistant":
        handleAssistant({ ...ctx, includeSidechainTurns });
        break;
      case "summary":
        handleSummary(ctx);
        break;
      case "system":
        handleSystem(ctx);
        break;
      case "cost-state":
        state.oneMillion ||= hasOneMillionKey(o);
        break;
      default:
        if (type && !KNOWN_IGNORED_TYPES.has(type)) {
          bump(state.unknownTypes, type);
        }
        break;
    }
  });

  const turns = [...state.turnsById.values()];
  const sidechain = [...state.sidechainById.values()];
  return measureTokens({
    provider: "claude-code",
    sessionId,
    path,
    models: [...state.models],
    events: measureThinking(state.events, turns),
    turns,
    compactionIndexes: state.compactionIndexes,
    subagents: [],
    ...opt("cwd", meta.cwd),
    ...opt("gitBranch", meta.gitBranch),
    ...opt("title", meta.title),
    ...opt("version", meta.version),
    ...opt("startedAt", meta.startedAt),
    ...opt("endedAt", meta.endedAt),
    ...opt("nativeContextWindow", state.oneMillion ? ONE_MILLION : undefined),
    ...opt("unknownRecordTypes", countsRecord(state.unknownTypes)),
    ...opt("unknownSystemSubtypes", countsRecord(state.unknownSubtypes)),
    ...opt(
      "sidechainCost",
      sidechain.length > 0 ? turnsCost(sidechain) : undefined
    ),
  });
};
