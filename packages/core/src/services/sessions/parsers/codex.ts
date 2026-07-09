/** OpenAI Codex CLI rollout parser: normalizes ~/.codex rollout JSONL into a
 * `ParsedSession`.
 *
 * Codex logs a rollout envelope per line — `{timestamp, type, payload}` where
 * `type` ∈ session_meta | response_item | turn_context | event_msg. The timeline
 * is built from `response_item` lines ONLY; `event_msg` is a UI/telemetry mirror
 * consumed solely for turns/usage/context-window. See the format spec.
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

/** A raw JSONL line, untyped. */
type RawLine = Record<string, unknown>;

/** Mutable turn used while building (schema `Turn` has readonly eventIndexes). */
type MutTurn = Omit<Turn, "eventIndexes"> & { eventIndexes: number[] };

const WHITESPACE = /\s+/g;

/** Matches a non-zero shell exit code in tool output, for the isError heuristic. */
const ERR_EXIT = /exit(?:ed with)? code:? ?[1-9]/i;

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
  p ? (p.split("/").filter(Boolean).pop() ?? "") : "";

/** Pretty-print a JSON string if it parses, else return it verbatim. */
const prettyJson = (s: string): string => {
  try {
    return JSON.stringify(JSON.parse(s), null, 2);
  } catch {
    return s;
  }
};

/** Concatenate the `text` parts of a `message.content` array. */
const contentText = (content: unknown): string => {
  if (!Array.isArray(content)) {
    return "";
  }
  return content
    .map((part) => {
      const o = asObj(part);
      return o && typeof o.text === "string" ? o.text : "";
    })
    .filter(Boolean)
    .join("\n");
};

/** Fields shared by every timeline event built here. */
interface EvBase {
  readonly index: number;
  readonly ts?: string;
}

/** Build a tool-call event from a response_item tool payload. */
const toolCall = (
  base: EvBase,
  name: string,
  callId: unknown,
  body: string
): TimelineEvent => ({
  ...base,
  kind: "tool-call",
  title: name || "tool",
  preview: firstLine(body.replace(WHITESPACE, " ")),
  body,
  tokensEst: estTokens(body),
  toolName: name || "tool",
  ...opt("toolUseId", typeof callId === "string" ? callId : undefined),
});

/** Build a tool-result event from a response_item `*_output` payload. */
const toolResult = (
  base: EvBase,
  callId: unknown,
  body: string
): TimelineEvent => {
  const isError = ERR_EXIT.test(body);
  return {
    ...base,
    kind: "tool-result",
    title: `tool_result${isError ? " (error)" : ""}`,
    preview: firstLine(body),
    body,
    tokensEst: estTokens(body),
    isError,
    ...opt("toolUseId", typeof callId === "string" ? callId : undefined),
  };
};

/** Build a timeline event from one `response_item` payload, or null to drop it. */
const responseEvent = (
  payload: Record<string, unknown>,
  base: EvBase
): TimelineEvent | null => {
  const ptype = str(payload.type);
  switch (ptype) {
    case "message": {
      const role = str(payload.role);
      const body = contentText(payload.content);
      if (role === "assistant") {
        return {
          ...base,
          kind: "assistant-text",
          title: "Assistant",
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
        };
      }
      if (role === "developer") {
        return {
          ...base,
          kind: "system",
          title: "developer instructions",
          attachmentType: "developer_instructions",
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
        };
      }
      const trimmed = body.trimStart();
      const injected =
        trimmed.startsWith("<environment_context>") ||
        trimmed.startsWith("<user_instructions>");
      if (injected) {
        return {
          ...base,
          kind: "system",
          title: "environment_context",
          attachmentType: "environment_context",
          preview: firstLine(body),
          body,
          tokensEst: estTokens(body),
        };
      }
      return {
        ...base,
        kind: "user-prompt",
        title: "User prompt",
        preview: firstLine(body),
        body,
        tokensEst: estTokens(body),
      };
    }
    case "reasoning": {
      const summary = Array.isArray(payload.summary) ? payload.summary : [];
      const text = summary
        .map((s) => {
          const o = asObj(s);
          return o && o.type === "summary_text" && typeof o.text === "string"
            ? o.text
            : "";
        })
        .filter(Boolean)
        .join("\n");
      return {
        ...base,
        kind: "assistant-thinking",
        title: "Thinking",
        preview: text ? firstLine(text) : "(reasoning encrypted — not stored)",
        body: text,
        tokensEst: estTokens(text),
      };
    }
    case "function_call":
      return toolCall(
        base,
        str(payload.name),
        payload.call_id,
        prettyJson(str(payload.arguments))
      );
    case "custom_tool_call":
      return toolCall(
        base,
        str(payload.name),
        payload.call_id,
        str(payload.input)
      );
    case "tool_search_call":
      return toolCall(
        base,
        "tool_search",
        payload.call_id,
        JSON.stringify(payload.arguments ?? {})
      );
    case "web_search_call": {
      const action = asObj(payload.action);
      return toolCall(
        base,
        "web_search",
        payload.call_id,
        action ? str(action.query) : ""
      );
    }
    case "function_call_output":
    case "custom_tool_call_output":
      return toolResult(base, payload.call_id, str(payload.output));
    case "tool_search_output":
      return toolResult(
        base,
        payload.call_id,
        JSON.stringify(payload.tools ?? [])
      );
    default:
      return null;
  }
};

/** Mutable session-level metadata gathered while scanning lines. */
interface Meta {
  cwd?: string;
  endedAt?: string;
  gitBranch?: string;
  sessionId?: string;
  startedAt?: string;
  version?: string;
}

/** Mutable scan cursor for turn attribution across the rollout. */
interface Cursor {
  currentModel: string;
  currentTurnId?: string;
  nativeWindow?: number;
  pending: number[];
}

/** Apply a `session_meta` payload to session metadata. */
const applySessionMeta = (meta: Meta, payload: Record<string, unknown>) => {
  const id = str(payload.id);
  if (id) {
    meta.sessionId = id;
  }
  if (typeof payload.cwd === "string") {
    meta.cwd ??= payload.cwd;
  }
  const git = asObj(payload.git);
  if (git && typeof git.branch === "string") {
    meta.gitBranch = git.branch;
  }
  if (typeof payload.cli_version === "string") {
    meta.version = payload.cli_version;
  }
};

/** Register one Turn from a token_count `info` block; reset the pending window. */
const applyTokenCount = (args: {
  readonly info: Record<string, unknown>;
  readonly cursor: Cursor;
  readonly counters: Map<string, number>;
  readonly turns: MutTurn[];
  readonly index: number;
  readonly ts: string | undefined;
}) => {
  const { info, cursor, counters, turns, index, ts } = args;
  const window = info.model_context_window;
  if (typeof window === "number") {
    cursor.nativeWindow = window;
  }
  const last = asObj(info.last_token_usage);
  if (last) {
    const turnId = cursor.currentTurnId ?? `tc-${index}`;
    const n = (counters.get(turnId) ?? 0) + 1;
    counters.set(turnId, n);
    const contextTokens = num(last.input_tokens);
    const cacheReadTokens = num(last.cached_input_tokens);
    if (contextTokens > 0) {
      turns.push({
        requestId: `${turnId}#${n}`,
        model: cursor.currentModel,
        contextTokens,
        inputTokens: contextTokens - cacheReadTokens,
        cacheReadTokens,
        cacheCreationTokens: 0,
        outputTokens: num(last.output_tokens),
        eventIndexes: cursor.pending.slice(),
        ...opt("ts", ts),
      });
    }
  }
  cursor.pending = [];
};

/** Apply a `turn_context` payload: track current model + turn id + cwd. */
const applyTurnContext = (args: {
  readonly payload: Record<string, unknown>;
  readonly cursor: Cursor;
  readonly models: Set<string>;
  readonly meta: Meta;
}) => {
  const { payload, cursor, models, meta } = args;
  if (typeof payload.model === "string") {
    cursor.currentModel = payload.model;
    models.add(payload.model);
  }
  if (typeof payload.turn_id === "string") {
    cursor.currentTurnId = payload.turn_id;
  }
  if (typeof payload.cwd === "string") {
    meta.cwd ??= payload.cwd;
  }
};

/** Apply an `event_msg` payload: turn boundaries + token_count-driven Turns. */
const applyEventMsg = (args: {
  readonly payload: Record<string, unknown>;
  readonly cursor: Cursor;
  readonly counters: Map<string, number>;
  readonly turns: MutTurn[];
  readonly index: number;
  readonly ts: string | undefined;
}) => {
  const { payload, cursor, counters, turns, index, ts } = args;
  const ptype = str(payload.type);
  if (ptype === "task_started") {
    if (typeof payload.turn_id === "string") {
      cursor.currentTurnId = payload.turn_id;
    }
    if (typeof payload.model_context_window === "number") {
      cursor.nativeWindow = payload.model_context_window;
    }
    return;
  }
  if (ptype === "token_count") {
    const info = asObj(payload.info);
    if (info) {
      applyTokenCount({ info, cursor, counters, turns, index, ts });
    }
  }
};

/** Fold one rollout line into the running parse state. */
const applyLine = (args: {
  readonly line: RawLine;
  readonly index: number;
  readonly events: TimelineEvent[];
  readonly turns: MutTurn[];
  readonly models: Set<string>;
  readonly counters: Map<string, number>;
  readonly meta: Meta;
  readonly cursor: Cursor;
}) => {
  const { line, index, events, turns, models, counters, meta, cursor } = args;
  const type = str(line.type);
  const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
  if (ts) {
    meta.startedAt ??= ts;
    meta.endedAt = ts;
  }
  const payload = asObj(line.payload) ?? {};
  switch (type) {
    case "session_meta":
      applySessionMeta(meta, payload);
      break;
    case "turn_context":
      applyTurnContext({ payload, cursor, models, meta });
      break;
    case "response_item": {
      const ev = responseEvent(payload, { index, ...opt("ts", ts) });
      if (ev) {
        events.push(ev);
        cursor.pending.push(events.length - 1);
      }
      break;
    }
    case "event_msg":
      applyEventMsg({ payload, cursor, counters, turns, index, ts });
      break;
    default:
      break;
  }
};

/**
 * Parse a Codex CLI rollout transcript into a `ParsedSession`.
 * Turn usage is taken verbatim from each `token_count.info.last_token_usage`
 * (per-call DELTA); event body sizes are chars/4 via `estTokens`.
 */
export const parseCodexSession = ({
  text,
  path,
  sessionId,
}: ParseSessionArgs): ParsedSession => {
  const events: TimelineEvent[] = [];
  const turns: MutTurn[] = [];
  const models = new Set<string>();
  const counters = new Map<string, number>();
  const meta: Meta = {};
  const cursor: Cursor = { currentModel: "unknown", pending: [] };

  parseJsonl(text).forEach((line, index) => {
    applyLine({ line, index, events, turns, models, counters, meta, cursor });
  });

  return {
    provider: "codex",
    sessionId: meta.sessionId ?? sessionId,
    path,
    models: [...models],
    events,
    turns,
    compactionIndexes: [],
    subagents: [],
    ...opt("cwd", meta.cwd),
    ...opt("gitBranch", meta.gitBranch),
    ...opt("version", meta.version),
    ...opt("startedAt", meta.startedAt),
    ...opt("endedAt", meta.endedAt),
    ...opt("nativeContextWindow", cursor.nativeWindow),
  };
};

/** Mutable header fields gathered while lazily scanning lines. */
interface HeaderAcc {
  cwd?: string;
  gitBranch?: string;
  model?: string;
  startedAt?: string;
  updatedAt?: string;
}

/** Fold one raw rollout line's header-relevant fields into the accumulator. */
const applyHeaderLine = (acc: HeaderAcc, line: RawLine) => {
  const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
  if (ts) {
    acc.startedAt ??= ts;
    acc.updatedAt = ts;
  }
  const type = str(line.type);
  const payload = asObj(line.payload);
  if (!payload) {
    return;
  }
  if (type === "session_meta") {
    if (typeof payload.cwd === "string") {
      acc.cwd ??= payload.cwd;
    }
    const git = asObj(payload.git);
    if (git && typeof git.branch === "string") {
      acc.gitBranch ??= git.branch;
    }
  }
  if (type === "turn_context") {
    if (typeof payload.cwd === "string") {
      acc.cwd ??= payload.cwd;
    }
    if (!acc.model && typeof payload.model === "string") {
      acc.model = payload.model;
    }
  }
};

/**
 * Build a lightweight Codex list header from raw rollout text. Scans lines for
 * cwd/branch/model/timestamps without constructing a timeline; the project name
 * is the cwd basename (Codex has no per-project slug dir).
 */
export const buildCodexHeader = ({
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
    messageCount += 1;
    try {
      applyHeaderLine(acc, JSON.parse(raw) as RawLine);
    } catch {
      /* tolerate a partial last line of a live rollout */
    }
  }

  return {
    id,
    agent: "codex",
    path,
    project: basename(acc.cwd) || slug || id,
    messageCount,
    sizeBytes,
    updatedAt: acc.updatedAt ?? new Date(mtimeMs).toISOString(),
    ...opt("cwd", acc.cwd),
    ...opt("gitBranch", acc.gitBranch),
    ...opt("model", acc.model),
    ...opt("startedAt", acc.startedAt),
  };
};

/** The Codex `SessionParser`. */
export const codexParser: SessionParser = {
  agent: "codex",
  parseSession: parseCodexSession,
  buildHeader: buildCodexHeader,
};
