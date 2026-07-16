/** OpenCode session parser — normalizes the OpenCode "dialect" JSONL into a
 * `ParsedSession`.
 *
 * OpenCode stores sessions in a SQLite DB (+ a legacy JSON tree); the node-only
 * `opencode/reader.ts` serializes one session into a self-defined JSONL dialect
 * so THIS parser stays pure over text, mirroring how Codex/Pi parse their own
 * transcripts. The dialect has three line kinds:
 *   - `session` : one header line (id, directory, model, tokens, times).
 *   - `message` : a hydrated message `{...data, id, sessionID}` with a role.
 *   - `part`    : a hydrated part `{...data, id, sessionID, messageID}` that
 *                 immediately follows its owning message line.
 *
 * Per-turn token usage is taken from an assistant message's `tokens` block when
 * present, else estimated; those per-turn numbers drive every metric. The
 * context window is left unset — `analyze` infers it from the model, exactly
 * like Pi.
 */
import { parseJsonl } from "../parse";
import type {
  ParsedSession,
  SessionHeader,
  TimelineEvent,
  Turn,
} from "../schema";
import { estTokens, firstLine } from "../tokens";
import type { BuildHeaderArgs, ParseSessionArgs, SessionParser } from "./types";

/** A raw dialect line, untyped. */
type RawLine = Record<string, unknown>;

/** Mutable turn used while building (schema `Turn` has readonly eventIndexes). */
type MutTurn = Omit<Turn, "eventIndexes"> & { eventIndexes: number[] };

const WHITESPACE = /\s+/g;
const PATH_SEP = /[\\/]/;

/** Spread helper that drops a key when its value is undefined (exactOptional safe). */
const opt = <K extends string, V>(
  key: K,
  value: V | undefined
): Partial<Record<K, V>> =>
  value === undefined ? {} : ({ [key]: value } as Record<K, V>);

/** Coerce an unknown into a plain object, or `undefined`. */
const asObj = (v: unknown): Record<string, unknown> | undefined =>
  v && typeof v === "object" && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : undefined;

/** Stringify an unknown value: pass strings through, JSON the rest. */
const str = (v: unknown): string => {
  if (typeof v === "string") {
    return v;
  }
  return v == null ? "" : JSON.stringify(v);
};

/** Read a numeric field, defaulting to 0 when absent/non-numeric. */
const num = (v: unknown): number => (typeof v === "number" ? v : 0);

/** Basename of a POSIX path (last non-empty segment), or "". */
const basename = (p: string | undefined): string =>
  p ? (p.split(PATH_SEP).filter(Boolean).pop() ?? "") : "";

/** Pretty-print a JSON value as 2-space JSON, or "" when nullish. */
const prettyJson = (v: unknown): string =>
  v == null ? "" : JSON.stringify(v, null, 2);

/** ISO string from an ms epoch, or `undefined` when not a positive number. */
const isoFromMs = (ms: unknown): string | undefined =>
  typeof ms === "number" && ms > 0 ? new Date(ms).toISOString() : undefined;

/** Extract a model id string from a session `model` (string or `{id}` object). */
const modelId = (model: unknown): string | undefined => {
  if (typeof model === "string") {
    const parsed = asObj(safeJson(model));
    return parsed && typeof parsed.id === "string" ? parsed.id : model;
  }
  const obj = asObj(model);
  return obj && typeof obj.id === "string" ? obj.id : undefined;
};

/** Parse a JSON string, or return `undefined` (never throws). */
const safeJson = (v: string): unknown => {
  try {
    return JSON.parse(v);
  } catch {
    return;
  }
};

/** The model id carried on a message's `model` / `modelID` field, if any. */
const messageModel = (msg: RawLine): string | undefined => {
  if (typeof msg.modelID === "string") {
    return msg.modelID;
  }
  const model = asObj(msg.model);
  if (model && typeof model.modelID === "string") {
    return model.modelID;
  }
  return model && typeof model.id === "string" ? model.id : undefined;
};

/** Truthful ms-epoch creation time of a message (from its `time.created`). */
const messageCreatedMs = (msg: RawLine): number | undefined => {
  const time = asObj(msg.time);
  return time && typeof time.created === "number" ? time.created : undefined;
};

/** Fields shared by every timeline event built here. */
interface EvBase {
  readonly index: number;
  readonly requestId?: string;
  readonly ts?: string;
}

/** Build the tool-call + tool-result pair for one `tool` part. */
const toolEvents = (part: RawLine, base: EvBase): TimelineEvent[] => {
  const toolName = str(part.tool) || "tool";
  const callId = typeof part.callID === "string" ? part.callID : undefined;
  const state = asObj(part.state) ?? {};
  const input = state.input;
  const inputBody = prettyJson(input);
  const call: TimelineEvent = {
    ...base,
    kind: "tool-call",
    title: toolName,
    preview: firstLine(inputBody.replace(WHITESPACE, " ")),
    body: inputBody,
    tokensEst: estTokens(inputBody),
    toolName,
    ...opt("toolUseId", callId),
  };
  const status = str(state.status);
  const isTerminal = status === "completed" || status === "error";
  if (!(isTerminal || typeof state.output === "string")) {
    return [call];
  }
  const isError = status === "error";
  const resultBody =
    typeof state.output === "string" ? state.output : prettyJson(state);
  const result: TimelineEvent = {
    ...base,
    kind: "tool-result",
    title: `tool_result${isError ? " (error)" : ""}`,
    preview: firstLine(resultBody),
    body: resultBody,
    tokensEst: estTokens(resultBody),
    isError,
    toolName,
    ...opt("toolUseId", callId),
  };
  return [call, result];
};

/** Build the timeline event(s) for one part, given the current message role. */
const partEvents = (args: {
  readonly part: RawLine;
  readonly role: string;
  readonly base: EvBase;
}): TimelineEvent[] => {
  const { part, role, base } = args;
  const type = str(part.type);
  switch (type) {
    case "text": {
      const text = str(part.text);
      return [
        {
          ...base,
          kind: role === "user" ? "user-prompt" : "assistant-text",
          title: role === "user" ? "User prompt" : "Assistant",
          preview: firstLine(text),
          body: text,
          tokensEst: estTokens(text),
        },
      ];
    }
    case "reasoning": {
      const text = str(part.text);
      return [
        {
          ...base,
          kind: "assistant-thinking",
          title: "Thinking",
          preview: text ? firstLine(text) : "(reasoning not stored)",
          body: text,
          tokensEst: estTokens(text),
        },
      ];
    }
    case "tool":
      return toolEvents(part, base);
    case "file": {
      const body = prettyJson(part);
      return [
        {
          ...base,
          kind: "attachment",
          title: str(part.filename) || "file",
          attachmentType: "file",
          preview: firstLine(str(part.filename) || body),
          body,
          tokensEst: estTokens(body),
        },
      ];
    }
    case "patch":
    case "snapshot": {
      const body = prettyJson(part);
      return [
        {
          ...base,
          kind: "attachment",
          title: type,
          attachmentType: type,
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
        },
      ];
    }
    case "subtask":
    case "agent": {
      const body = prettyJson(part);
      return [
        {
          ...base,
          kind: "system",
          title: type === "subtask" ? "subagent spawn" : "agent",
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
          ...opt(
            "toolName",
            typeof part.tool === "string" ? part.tool : undefined
          ),
        },
      ];
    }
    case "retry": {
      const body = prettyJson(part);
      return [
        {
          ...base,
          kind: "system",
          title: "retry",
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
        },
      ];
    }
    case "compaction": {
      const body = prettyJson(part);
      return [
        {
          ...base,
          kind: "compaction",
          title: "Compaction",
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
        },
      ];
    }
    default:
      // step-start / step-finish and any unknown control part: drop.
      return [];
  }
};

/** Mutable session-level metadata gathered while scanning lines. */
interface Meta {
  cwd?: string;
  /** ISO of the last message seen — the endedAt fallback when no session end. */
  lastMessageAt?: string;
  /** ISO(timeUpdated) from the session line — authoritative when present. */
  sessionEndedAt?: string;
  sessionId?: string;
  sessionModel?: string;
  startedAt?: string;
  title?: string;
  version?: string;
}

/** Mutable scan cursor: current message context for part attribution. */
interface Cursor {
  contextRunning: number;
  msgCreatedMs: number | undefined;
  role: string;
  turn: MutTurn | undefined;
}

/** Register the Turn for an assistant message; returns it for event linking. */
const turnForMessage = (args: {
  readonly msg: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly sessionModel: string | undefined;
  readonly contextRunning: number;
}): MutTurn => {
  const { msg, index, ts, sessionModel, contextRunning } = args;
  const model = messageModel(msg) ?? sessionModel ?? "unknown";
  const requestId = typeof msg.id === "string" ? msg.id : `msg-${index}`;
  const tokens = asObj(msg.tokens);
  const inputTokens = num(tokens?.input);
  const outputTokens = num(tokens?.output);
  const cache = asObj(tokens?.cache);
  const cacheReadTokens = num(cache?.read);
  const cacheCreationTokens = num(cache?.write);
  const groundContext = inputTokens + cacheReadTokens + cacheCreationTokens;
  const contextTokens = groundContext > 0 ? groundContext : contextRunning;
  return {
    requestId,
    model,
    contextTokens,
    inputTokens,
    cacheReadTokens,
    cacheCreationTokens,
    outputTokens,
    eventIndexes: [],
    ...opt("ts", ts),
  };
};

/** Fold one message line + establish the cursor for its following parts. */
const applyMessage = (args: {
  readonly msg: RawLine;
  readonly index: number;
  readonly cursor: Cursor;
  readonly turns: MutTurn[];
  readonly models: Set<string>;
  readonly meta: Meta;
}) => {
  const { msg, index, cursor, turns, models, meta } = args;
  const role = str(msg.role);
  cursor.role = role;
  cursor.turn = undefined;
  const createdMs = messageCreatedMs(msg);
  cursor.msgCreatedMs = createdMs;
  const ts = isoFromMs(createdMs);
  if (ts) {
    meta.startedAt ??= ts;
    meta.lastMessageAt = ts;
  }
  const model = messageModel(msg);
  if (model) {
    models.add(model);
  }
  if (role === "assistant") {
    const turn = turnForMessage({
      msg,
      index,
      ts,
      sessionModel: meta.sessionModel,
      contextRunning: cursor.contextRunning,
    });
    turns.push(turn);
    cursor.turn = turn;
  }
};

/** Fold one part line into the timeline, linking it to the current turn. */
const applyPart = (args: {
  readonly part: RawLine;
  readonly index: number;
  readonly cursor: Cursor;
  readonly events: TimelineEvent[];
  readonly compactionIndexes: number[];
}) => {
  const { part, index, cursor, events, compactionIndexes } = args;
  const base: EvBase = {
    index,
    ...opt("ts", isoFromMs(messageCreatedMs(part) ?? cursor.msgCreatedMs)),
    ...opt("requestId", cursor.turn?.requestId),
  };
  for (const ev of partEvents({ part, role: cursor.role, base })) {
    const eventIndex = events.length;
    const withIndex: TimelineEvent = { ...ev, index: eventIndex };
    events.push(withIndex);
    cursor.contextRunning += withIndex.tokensEst;
    cursor.turn?.eventIndexes.push(eventIndex);
    if (withIndex.kind === "compaction") {
      compactionIndexes.push(eventIndex);
    }
  }
};

/** Apply the leading `session` line to session metadata. */
const applySession = (meta: Meta, line: RawLine, models: Set<string>) => {
  if (typeof line.id === "string") {
    meta.sessionId = line.id;
  }
  if (typeof line.directory === "string") {
    meta.cwd = line.directory;
  }
  if (typeof line.title === "string") {
    meta.title = line.title;
  }
  if (typeof line.version === "string") {
    meta.version = line.version;
  }
  const model = modelId(line.model);
  if (model) {
    meta.sessionModel = model;
    models.add(model);
  }
  const created = isoFromMs(line.timeCreated);
  if (created) {
    meta.startedAt ??= created;
  }
  const updated = isoFromMs(line.timeUpdated);
  if (updated) {
    meta.sessionEndedAt = updated;
  }
};

/**
 * Parse an OpenCode dialect transcript into a `ParsedSession`. Never throws on
 * malformed lines (tolerant decode via `parseJsonl`).
 */
export const parseOpencodeSession = ({
  text,
  path,
  sessionId,
}: ParseSessionArgs): ParsedSession => {
  const events: TimelineEvent[] = [];
  const turns: MutTurn[] = [];
  const models = new Set<string>();
  const compactionIndexes: number[] = [];
  const meta: Meta = {};
  const cursor: Cursor = {
    role: "",
    contextRunning: 0,
    msgCreatedMs: undefined,
    turn: undefined,
  };

  parseJsonl(text).forEach((line, index) => {
    const kind = str(line.kind);
    if (kind === "session") {
      applySession(meta, line, models);
      return;
    }
    if (kind === "message") {
      applyMessage({ msg: line, index, cursor, turns, models, meta });
      return;
    }
    if (kind === "part") {
      applyPart({ part: line, index, cursor, events, compactionIndexes });
    }
  });

  return {
    provider: "opencode",
    sessionId: meta.sessionId ?? sessionId,
    path,
    models: [...models],
    events,
    turns,
    compactionIndexes,
    subagents: [],
    ...opt("cwd", meta.cwd),
    ...opt("title", meta.title),
    ...opt("version", meta.version),
    ...opt("startedAt", meta.startedAt),
    ...opt("endedAt", meta.sessionEndedAt ?? meta.lastMessageAt),
  };
};

/** Mutable header fields gathered from the leading `session` line. */
interface HeaderAcc {
  cwd?: string;
  model?: string;
  startedAt?: string;
  title?: string;
  updatedAt?: string;
}

/** Read the header-relevant fields off the first `session` dialect line. */
const applyHeaderSession = (acc: HeaderAcc, line: RawLine) => {
  if (typeof line.directory === "string") {
    acc.cwd = line.directory;
  }
  if (typeof line.title === "string") {
    acc.title = line.title;
  }
  const model = modelId(line.model);
  if (model) {
    acc.model = model;
  }
  const started = isoFromMs(line.timeCreated);
  if (started) {
    acc.startedAt = started;
  }
  const updated = isoFromMs(line.timeUpdated);
  if (updated) {
    acc.updatedAt = updated;
  }
};

/**
 * Build a lightweight OpenCode list header from dialect text. The first line is
 * the `session` header, so meta is read cheaply; `messageCount` counts the
 * `message` lines and the project name is the directory basename (or the slug).
 */
export const buildOpencodeHeader = ({
  text,
  id,
  slug,
  path,
  sizeBytes,
  mtimeMs,
}: BuildHeaderArgs): SessionHeader => {
  const acc: HeaderAcc = {};
  let messageCount = 0;
  for (const raw of text.split("\n")) {
    if (!raw.trim()) {
      continue;
    }
    let line: RawLine;
    try {
      line = JSON.parse(raw) as RawLine;
    } catch {
      continue;
    }
    const kind = str(line.kind);
    if (kind === "session") {
      applyHeaderSession(acc, line);
    } else if (kind === "message") {
      messageCount += 1;
    }
  }

  return {
    id,
    agent: "opencode",
    path,
    project: basename(acc.cwd) || slug || id,
    messageCount,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...opt("cwd", acc.cwd),
    ...opt("model", acc.model),
    ...opt("title", acc.title),
    ...opt("startedAt", acc.startedAt),
  };
};

/** The OpenCode `SessionParser`. */
export const opencodeParser: SessionParser = {
  agent: "opencode",
  parseSession: parseOpencodeSession,
  buildHeader: buildOpencodeHeader,
};
