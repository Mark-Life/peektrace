/** Pi (pi.dev) session parser — normalizes `~/.pi/agent/sessions` JSONL.
 *
 * Top-level `.type`s: `session` (header), `model_change` /
 * `thinking_level_change` (control), `session_info` (title), `message` (the
 * conversation), and the context entries `compaction`, `branch_summary`,
 * `custom_message`, and `context_edit` (see `pi-edits.ts`). Conversation lines discriminate on `.message.role`
 * (`system` | `user` | `assistant` | `toolResult` plus Pi's custom roles) —
 * there is no `.message.type`. Token usage is ground-truth from `.message.usage`; body
 * sizes are chars/4 via `estTokens`. Mirrors the Claude parser's tolerant,
 * per-block, exactOptional-safe style.
 */

import { imageMarker, isImageBlock } from "../images";
import { parseJsonl } from "../parse";
import type { ParsedSession, TimelineEvent, Turn } from "../schema";
import { estTokens, firstLine } from "../tokens";
import { windowForModel } from "./model-windows";
import {
  type PiBase,
  piBashEvent,
  piBranchSummaryEvent,
  piCompactionEvent,
  piCustomEvent,
  piSystemEvent,
  piToolResultText,
  str,
} from "./pi-context";
import { collectPiEdits, foldWithEdit, isEditable } from "./pi-edits";
import { buildPiHeader } from "./pi-header";
import type { ParseSessionArgs, SessionParser } from "./types";

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

interface ParseState {
  readonly compactionIndexes: number[];
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
  /** A system message was already shown; later ones are patches. */
  systemSeen?: boolean;
  title?: string;
  version?: string;
}

/** Build a user content block into a `user-prompt` event, if it carries text. */
const userBlockEvent = (args: {
  readonly block: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
}): TimelineEvent | null => {
  const { block, index, ts } = args;
  if (isImageBlock(block)) {
    const text = imageMarker(block);
    return {
      index,
      kind: "attachment",
      title: "image",
      preview: text,
      body: text,
      tokensEst: estTokens(text),
      attachmentType: "image",
      loadedCategory: "file",
      ...opt("ts", ts),
    };
  }
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
  const content =
    typeof msg.content === "string"
      ? [{ type: "text", text: msg.content }]
      : msg.content;
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
  const text = piToolResultText(msg.content);
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

/** Push an optional event. */
const pushEvent = (state: ParseState, ev: TimelineEvent | null) => {
  if (ev) {
    state.events.push(ev);
  }
};

/** Push a compaction event and remember its line. */
const pushCompaction = (state: ParseState, base: PiBase, summary: unknown) => {
  state.compactionIndexes.push(base.index);
  state.events.push(piCompactionEvent(base, summary));
};

/** Push a system message event; the first one is the prompt, the rest patch it. */
const pushSystem = (args: {
  readonly state: ParseState;
  readonly meta: Meta;
  readonly msg: RawLine;
  readonly base: PiBase;
}) => {
  const { state, meta, msg, base } = args;
  const ev = piSystemEvent({ msg, base, first: meta.systemSeen !== true });
  if (ev) {
    meta.systemSeen = true;
    state.events.push(ev);
  }
};

/** Fold a message with one of Pi's non-standard roles into events. */
const handleExtraRole = (args: {
  readonly role: string;
  readonly msg: RawLine;
  readonly base: PiBase;
  readonly state: ParseState;
  readonly meta: Meta;
}) => {
  const { role, msg, base, state, meta } = args;
  switch (role) {
    case "system":
      pushSystem({ state, meta, msg, base });
      return;
    case "custom":
      pushEvent(state, piCustomEvent(base, msg));
      return;
    case "bashExecution":
      pushEvent(state, piBashEvent(base, msg));
      return;
    case "branchSummary":
      state.events.push(piBranchSummaryEvent(base, msg.summary));
      return;
    case "compactionSummary":
      pushCompaction(state, base, msg.summary);
      return;
    default:
      return;
  }
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
    return;
  }
  handleExtraRole({ role, msg, base: { index, ts }, state, meta });
};

/** Fold a top-level context entry (`compaction`, `branch_summary`, `custom_message`). */
const handleContextEntry = (args: {
  readonly type: string;
  readonly line: RawLine;
  readonly base: PiBase;
  readonly state: ParseState;
  readonly meta: Meta;
}) => {
  const { type, line, base, state, meta } = args;
  if (type === "compaction") {
    // The new system message applies after the compaction, so it goes below it.
    pushCompaction(state, base, line.summary);
    const sys = line.systemMessage;
    if (sys && typeof sys === "object") {
      pushSystem({ state, meta, msg: sys as RawLine, base });
    }
  } else if (type === "branch_summary") {
    state.events.push(piBranchSummaryEvent(base, line.summary));
  } else if (type === "custom_message") {
    pushEvent(state, piCustomEvent(base, line));
  }
};

/** Read id, cwd and version from the `session` header line. */
const applySessionLine = (meta: Meta, line: RawLine) => {
  if (typeof line.id === "string") {
    meta.sessionId = line.id;
  }
  if (typeof line.cwd === "string") {
    meta.cwd = line.cwd;
  }
  if (line.version != null) {
    meta.version = String(line.version);
  }
};

/**
 * Parse a Pi (pi.dev) transcript into a ParsedSession.
 * Token usage is taken verbatim from `.message.usage`; body sizes are chars/4.
 */
export const parsePiSession = (args: ParseSessionArgs): ParsedSession => {
  const { text, path, sessionId } = args;
  const state: ParseState = {
    compactionIndexes: [],
    events: [],
    turnsById: new Map(),
    models: new Set(),
  };
  const meta: Meta = {};

  const lines = parseJsonl(text);
  const edits = collectPiEdits(lines);
  const editFor = (line: RawLine) =>
    typeof line.id === "string" && isEditable(line)
      ? edits.get(line.id)
      : undefined;

  lines.forEach((line, index) => {
    const type = String(line.type ?? "");
    const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
    if (ts) {
      meta.startedAt ??= ts;
      meta.endedAt = ts;
    }
    switch (type) {
      case "session":
        applySessionLine(meta, line);
        break;
      case "model_change": {
        if (typeof line.modelId === "string") {
          state.models.add(line.modelId);
          meta.lastModel = line.modelId;
        }
        break;
      }
      case "session_info":
        if (typeof line.name === "string" && line.name.trim() !== "") {
          meta.title = line.name;
        }
        break;
      case "message":
        foldWithEdit({
          state,
          line,
          edits: editFor(line),
          run: (l) => handleMessage({ line: l, index, ts, state, meta }),
        });
        break;
      default:
        foldWithEdit({
          state,
          line,
          edits: editFor(line),
          run: (l) =>
            handleContextEntry({
              type,
              line: l,
              base: { index, ts },
              state,
              meta,
            }),
        });
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
    compactionIndexes: state.compactionIndexes,
    subagents: [],
    ...opt("cwd", meta.cwd),
    ...opt("title", meta.title),
    ...opt("version", meta.version),
    ...opt("startedAt", meta.startedAt),
    ...opt("endedAt", meta.endedAt),
    ...opt("nativeContextWindow", nativeContextWindow),
  };
};

/** The Pi `SessionParser`. */
export const piParser: SessionParser = {
  agent: "pi",
  parseSession: parsePiSession,
  buildHeader: buildPiHeader,
};
