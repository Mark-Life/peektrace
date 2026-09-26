/** Parse a Claude Code JSONL transcript into a normalized ParsedSession. */
import { attachmentEvent, type PromptSeen } from "./claude-attachments";
import { imageMarker, isImageBlock, omitImageData } from "./images";
import type { ParsedSession, TimelineEvent, Turn } from "./schema";
import { estTokens, firstLine } from "./tokens";

/** A raw JSONL line, untyped. */
type RawLine = Record<string, unknown>;

/** Mutable turn used while building (schema `Turn` has readonly eventIndexes). */
type MutTurn = Omit<Turn, "eventIndexes"> & { eventIndexes: number[] };

const WHITESPACE = /\s+/g;

/** Spread helper that drops a key when its value is undefined (exactOptional safe). */
const opt = <K extends string, V>(
  key: K,
  value: V | undefined
): Partial<Record<K, V>> =>
  value === undefined ? {} : ({ [key]: value } as Record<K, V>);

/** Parse JSONL text into raw lines, skipping blanks and malformed rows. */
export const parseJsonl = (text: string): RawLine[] => {
  const out: RawLine[] = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) {
      continue;
    }
    try {
      out.push(JSON.parse(line) as RawLine);
    } catch {
      /* tolerate a partial last line of a live session */
    }
  }
  return out;
};

const str = (v: unknown): string => {
  if (typeof v === "string") {
    return v;
  }
  return v == null ? "" : JSON.stringify(v);
};

interface ParseState {
  readonly compactionIndexes: number[];
  readonly events: TimelineEvent[];
  readonly models: Set<string>;
  /** Prompt parts already shown, per thread (main or one subagent). */
  readonly promptSeen: Map<string, PromptSeen>;
  readonly turnsById: Map<string, MutTurn>;
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
  readonly isMeta: boolean;
}): TimelineEvent | null => {
  const { block, index, ts, isSidechain, isMeta } = args;
  if (block?.type === "tool_result") {
    const raw = omitImageData(block.content);
    const text = typeof raw === "string" ? raw : str(raw);
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
      ...opt(
        "toolUseId",
        typeof block.tool_use_id === "string" ? block.tool_use_id : undefined
      ),
    };
  }
  if (block?.type === "text") {
    const text = str(block.text);
    return {
      index,
      kind: "user-prompt",
      isSidechain,
      title: "User message",
      preview: firstLine(text),
      body: text,
      tokensEst: estTokens(text),
      ...opt("ts", ts),
      ...opt("isMeta", isMeta ? true : undefined),
    };
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
      ...opt("ts", ts),
    };
  }
  return null;
};

/** Build an assistant content event (thinking / text / tool_use). */
const assistantBlockEvent = (args: {
  readonly block: Record<string, unknown>;
  readonly index: number;
  readonly ts: string | undefined;
  readonly isSidechain: boolean;
  readonly requestId: string;
}): TimelineEvent | null => {
  const { block, index, ts, isSidechain, requestId } = args;
  const base = { index, requestId, isSidechain, ...opt("ts", ts) };
  if (block?.type === "thinking") {
    const text = str(block.thinking);
    return {
      ...base,
      kind: "assistant-thinking",
      title: "Thinking",
      preview: text ? firstLine(text) : "(content not stored in transcript)",
      body: text,
      tokensEst: estTokens(text),
    };
  }
  if (block?.type === "text") {
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
  if (block?.type === "tool_use") {
    const inputStr = JSON.stringify(block.input ?? {}, null, 2);
    return {
      ...base,
      kind: "tool-call",
      title: String(block.name ?? "tool"),
      preview: firstLine(inputStr.replace(WHITESPACE, " ")),
      body: inputStr,
      tokensEst: estTokens(inputStr),
      toolName: String(block.name ?? "tool"),
      ...opt("toolUseId", typeof block.id === "string" ? block.id : undefined),
    };
  }
  return null;
};

/** Handle one `user` line, appending its events. */
const handleUser = (ctx: LineCtx) => {
  const { o, index, ts, isSidechain, state } = ctx;
  const msg = (o.message ?? {}) as Record<string, unknown>;
  const content = msg.content;
  const isCompact = o.isCompactSummary === true;
  const isMeta = o.isMeta === true;
  if (isCompact) {
    state.compactionIndexes.push(index);
  }
  if (typeof content === "string") {
    state.events.push({
      index,
      kind: isCompact ? "compaction" : "user-prompt",
      isSidechain,
      title: isCompact ? "Context compaction (summary)" : "User prompt",
      preview: firstLine(content),
      body: content,
      tokensEst: estTokens(content),
      ...opt("ts", ts),
      ...opt("isMeta", isMeta ? true : undefined),
    });
    return;
  }
  if (Array.isArray(content)) {
    for (const block of content as Record<string, unknown>[]) {
      const ev = userBlockEvent({ block, index, ts, isSidechain, isMeta });
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
  readonly usage: Record<string, number>;
  readonly countable: boolean;
}) => {
  const { state, requestId, model, ts, usage, countable } = args;
  const inputTokens = usage.input_tokens ?? 0;
  const cacheRead = usage.cache_read_input_tokens ?? 0;
  const cacheCreate = usage.cache_creation_input_tokens ?? 0;
  const contextTokens = inputTokens + cacheRead + cacheCreate;
  if (!state.turnsById.has(requestId) && contextTokens > 0 && countable) {
    state.turnsById.set(requestId, {
      requestId,
      model,
      contextTokens,
      inputTokens,
      cacheReadTokens: cacheRead,
      cacheCreationTokens: cacheCreate,
      outputTokens: usage.output_tokens ?? 0,
      eventIndexes: [],
      ...opt("ts", ts),
    });
  }
};

/** Handle one `assistant` line: register the turn, append content events. */
const handleAssistant = (
  ctx: LineCtx & { readonly includeSidechainTurns: boolean }
) => {
  const { o, index, ts, isSidechain, state, includeSidechainTurns } = ctx;
  const msg = (o.message ?? {}) as Record<string, unknown>;
  const model = String(msg.model ?? "unknown");
  state.models.add(model);
  const requestId =
    typeof o.requestId === "string" ? o.requestId : `line-${index}`;
  registerTurn({
    state,
    requestId,
    model,
    ts,
    usage: (msg.usage ?? {}) as Record<string, number>,
    countable: includeSidechainTurns || !isSidechain,
  });
  const turn = state.turnsById.get(requestId);
  for (const block of (msg.content ?? []) as Record<string, unknown>[]) {
    const ev = assistantBlockEvent({
      block,
      index,
      ts,
      isSidechain,
      requestId,
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

export interface ParseClaudeArgs {
  /** Count `isSidechain` turns toward the curve (on only for a subagent's own file). */
  readonly includeSidechainTurns?: boolean;
  readonly path: string;
  readonly sessionId: string;
  readonly text: string;
}

/**
 * Parse a Claude Code transcript into a ParsedSession.
 * Token usage is taken verbatim from assistant `usage`; body sizes are chars/4.
 */
export const parseClaudeSession = (args: ParseClaudeArgs): ParsedSession => {
  const { text, path, sessionId, includeSidechainTurns = false } = args;
  const state: ParseState = {
    events: [],
    turnsById: new Map(),
    compactionIndexes: [],
    models: new Set(),
    promptSeen: new Map(),
  };
  const meta: Meta = {};

  parseJsonl(text).forEach((o, index) => {
    const type = String(o.type ?? "");
    const ts = typeof o.timestamp === "string" ? o.timestamp : undefined;
    applyMeta(meta, o, ts);
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
        handleAttachment(ctx);
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
      default:
        break;
    }
  });

  return {
    provider: "claude-code",
    sessionId,
    path,
    models: [...state.models],
    events: state.events,
    turns: [...state.turnsById.values()],
    compactionIndexes: state.compactionIndexes,
    subagents: [],
    ...opt("cwd", meta.cwd),
    ...opt("gitBranch", meta.gitBranch),
    ...opt("title", meta.title),
    ...opt("version", meta.version),
    ...opt("startedAt", meta.startedAt),
    ...opt("endedAt", meta.endedAt),
  };
};
