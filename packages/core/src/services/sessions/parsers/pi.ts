/** Pi (pi.dev) session parser — normalizes `~/.pi/agent/sessions` JSONL.
 *
 * Pi transcripts are JSONL with four top-level `.type`s: `session` (header),
 * `model_change` / `thinking_level_change` (control/meta), and `message`
 * (the conversation payload). Conversation lines discriminate on
 * `.message.role` (`user` | `assistant` | `toolResult`) — there is no
 * `.message.type`. Token usage is ground-truth from `.message.usage`; body
 * sizes are chars/4 via `estTokens`. Mirrors the Claude parser's tolerant,
 * per-block, exactOptional-safe style.
 */
import { parseJsonl } from "../parse";
import type {
  ParsedSession,
  SessionHeader,
  TimelineEvent,
  Turn,
} from "../schema";
import { estTokens, firstLine } from "../tokens";
import { windowForModel } from "./model-windows";
import type { BuildHeaderArgs, ParseSessionArgs, SessionParser } from "./types";

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

/** Coerce an unknown value into a display string (JSON for non-strings). */
const str = (v: unknown): string => {
  if (typeof v === "string") {
    return v;
  }
  return v == null ? "" : JSON.stringify(v);
};

/** Join a Pi content array of `{type:"text", text}` blocks into one body. */
const joinTextBlocks = (content: unknown): string => {
  if (typeof content === "string") {
    return content;
  }
  if (!Array.isArray(content)) {
    return str(content);
  }
  return (content as RawLine[])
    .map((b) => (typeof b?.text === "string" ? b.text : ""))
    .join("");
};

interface ParseState {
  readonly events: TimelineEvent[];
  readonly models: Set<string>;
  readonly turnsById: Map<string, MutTurn>;
}

/** Mutable session-level metadata gathered while scanning lines. */
interface Meta {
  cwd?: string;
  endedAt?: string;
  lastModel?: string;
  sessionId?: string;
  startedAt?: string;
  version?: string;
}

/** Build a user content block into a `user-prompt` event, if it carries text. */
const userBlockEvent = (args: {
  readonly block: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
}): TimelineEvent | null => {
  const { block, index, ts } = args;
  if (block?.type !== "text") {
    return null;
  }
  const text = str(block.text);
  return {
    index,
    kind: "user-prompt",
    title: "User message",
    preview: firstLine(text),
    body: text,
    tokensEst: estTokens(text),
    ...opt("ts", ts),
  };
};

/** Build an assistant content block into a thinking / text / tool-call event. */
const assistantBlockEvent = (args: {
  readonly block: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly requestId: string;
}): TimelineEvent | null => {
  const { block, index, ts, requestId } = args;
  const base = { index, requestId, ...opt("ts", ts) };
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
  if (block?.type === "toolCall") {
    const argsStr = JSON.stringify(block.arguments ?? {}, null, 2);
    return {
      ...base,
      kind: "tool-call",
      title: String(block.name ?? "tool"),
      preview: firstLine(argsStr.replace(WHITESPACE, " ")),
      body: argsStr,
      tokensEst: estTokens(argsStr),
      toolName: String(block.name ?? "tool"),
      ...opt("toolUseId", typeof block.id === "string" ? block.id : undefined),
    };
  }
  return null;
};

/** Append the `user-prompt` events for one user `message` line. */
const handleUser = (args: {
  readonly msg: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly state: ParseState;
}) => {
  const { msg, index, ts, state } = args;
  const content = msg.content;
  if (!Array.isArray(content)) {
    return;
  }
  for (const block of content as RawLine[]) {
    const ev = userBlockEvent({ block, index, ts });
    if (ev) {
      state.events.push(ev);
    }
  }
};

/** Register the turn for an assistant line when its usage occupies context. */
const registerTurn = (args: {
  readonly state: ParseState;
  readonly requestId: string;
  readonly model: string;
  readonly ts: string | undefined;
  readonly usage: Record<string, number>;
}) => {
  const { state, requestId, model, ts, usage } = args;
  const inputTokens = usage.input ?? 0;
  const cacheRead = usage.cacheRead ?? 0;
  const cacheWrite = usage.cacheWrite ?? 0;
  const contextTokens = inputTokens + cacheRead + cacheWrite;
  if (!state.turnsById.has(requestId) && contextTokens > 0) {
    state.turnsById.set(requestId, {
      requestId,
      model,
      contextTokens,
      inputTokens,
      cacheReadTokens: cacheRead,
      cacheCreationTokens: cacheWrite,
      outputTokens: usage.output ?? 0,
      eventIndexes: [],
      ...opt("ts", ts),
    });
  }
};

/** Append events + register the turn for one assistant `message` line. */
const handleAssistant = (args: {
  readonly line: RawLine;
  readonly msg: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly state: ParseState;
}) => {
  const { line, msg, index, ts, state } = args;
  const model = String(msg.model ?? "unknown");
  state.models.add(model);
  const requestId = typeof line.id === "string" ? line.id : `line-${index}`;
  registerTurn({
    state,
    requestId,
    model,
    ts,
    usage: (msg.usage ?? {}) as Record<string, number>,
  });
  const turn = state.turnsById.get(requestId);
  const content = Array.isArray(msg.content) ? (msg.content as RawLine[]) : [];
  for (const block of content) {
    const ev = assistantBlockEvent({ block, index, ts, requestId });
    if (ev) {
      state.events.push(ev);
      if (turn) {
        turn.eventIndexes.push(state.events.length - 1);
      }
    }
  }
};

/** Append the `tool-result` event for one toolResult `message` line. */
const handleToolResult = (args: {
  readonly msg: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly state: ParseState;
}) => {
  const { msg, index, ts, state } = args;
  const text = joinTextBlocks(msg.content);
  const isError = msg.isError === true;
  state.events.push({
    index,
    kind: "tool-result",
    title: `tool_result${isError ? " (error)" : ""}`,
    preview: firstLine(text),
    body: text,
    tokensEst: estTokens(text),
    isError,
    ...opt(
      "toolName",
      typeof msg.toolName === "string" ? msg.toolName : undefined
    ),
    ...opt(
      "toolUseId",
      typeof msg.toolCallId === "string" ? msg.toolCallId : undefined
    ),
    ...opt("ts", ts),
  });
};

/** Fold one `message` line's payload into events + turns. */
const handleMessage = (args: {
  readonly line: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly state: ParseState;
  readonly meta: Meta;
}) => {
  const { line, index, ts, state, meta } = args;
  const msg = (line.message ?? {}) as RawLine;
  const role = String(msg.role ?? "");
  if (role === "user") {
    handleUser({ msg, index, ts, state });
    return;
  }
  if (role === "assistant") {
    if (typeof msg.model === "string") {
      meta.lastModel = msg.model;
    }
    handleAssistant({ line, msg, index, ts, state });
    return;
  }
  if (role === "toolResult") {
    handleToolResult({ msg, index, ts, state });
  }
};

/**
 * Parse a Pi (pi.dev) transcript into a ParsedSession.
 * Token usage is taken verbatim from `.message.usage`; body sizes are chars/4.
 */
export const parsePiSession = (args: ParseSessionArgs): ParsedSession => {
  const { text, path, sessionId } = args;
  const state: ParseState = {
    events: [],
    turnsById: new Map(),
    models: new Set(),
  };
  const meta: Meta = {};

  parseJsonl(text).forEach((line, index) => {
    const type = String(line.type ?? "");
    const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
    if (ts) {
      meta.startedAt ??= ts;
      meta.endedAt = ts;
    }
    switch (type) {
      case "session": {
        if (typeof line.id === "string") {
          meta.sessionId = line.id;
        }
        if (typeof line.cwd === "string") {
          meta.cwd = line.cwd;
        }
        if (line.version != null) {
          meta.version = String(line.version);
        }
        break;
      }
      case "model_change": {
        if (typeof line.modelId === "string") {
          state.models.add(line.modelId);
          meta.lastModel = line.modelId;
        }
        break;
      }
      case "message":
        handleMessage({ line, index, ts, state, meta });
        break;
      default:
        break;
    }
  });

  const nativeContextWindow = windowForModel(meta.lastModel);
  return {
    provider: "pi",
    sessionId: meta.sessionId ?? sessionId,
    path,
    models: [...state.models],
    events: state.events,
    turns: [...state.turnsById.values()],
    compactionIndexes: [],
    subagents: [],
    ...opt("cwd", meta.cwd),
    ...opt("version", meta.version),
    ...opt("startedAt", meta.startedAt),
    ...opt("endedAt", meta.endedAt),
    ...opt("nativeContextWindow", nativeContextWindow),
  };
};

/** Mutable header fields gathered while scanning Pi lines. */
interface HeaderAcc {
  cwd?: string;
  model?: string;
  startedAt?: string;
  updatedAt?: string;
}

/** Fold one raw Pi line's header-relevant fields into the accumulator. */
const applyHeaderLine = (acc: HeaderAcc, line: RawLine) => {
  const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
  if (ts) {
    acc.startedAt ??= ts;
    acc.updatedAt = ts;
  }
  const type = String(line.type ?? "");
  if (type === "session" && typeof line.cwd === "string") {
    acc.cwd = line.cwd;
  }
  if (
    !acc.model &&
    type === "model_change" &&
    typeof line.modelId === "string"
  ) {
    acc.model = line.modelId;
  }
  if (!acc.model && type === "message") {
    const msg = (line.message ?? {}) as RawLine;
    if (msg.role === "assistant" && typeof msg.model === "string") {
      acc.model = msg.model;
    }
  }
};

/**
 * Build a lightweight Pi header from raw transcript text. Scans lines for the
 * cwd, first model, and timestamps; never constructs timeline events.
 */
export const buildPiHeader = (args: BuildHeaderArgs): SessionHeader => {
  const { text, id, slug, path, sizeBytes, mtimeMs } = args;
  const acc: HeaderAcc = {};
  let messageCount = 0;
  for (const raw of text.split("\n")) {
    if (!raw.trim()) {
      continue;
    }
    messageCount += 1;
    try {
      applyHeaderLine(acc, JSON.parse(raw) as RawLine);
    } catch {
      /* tolerate a partial last line of a live session */
    }
  }

  return {
    id,
    agent: "pi",
    path,
    project: slug,
    messageCount,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...opt("cwd", acc.cwd),
    ...opt("model", acc.model),
    ...opt("startedAt", acc.startedAt),
  };
};

/** The Pi `SessionParser`. */
export const piParser: SessionParser = {
  agent: "pi",
  parseSession: parsePiSession,
  buildHeader: buildPiHeader,
};
