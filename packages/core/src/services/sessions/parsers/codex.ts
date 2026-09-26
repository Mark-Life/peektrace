/** OpenAI Codex CLI rollout parser: normalizes ~/.codex rollout JSONL into a
 * `ParsedSession`.
 *
 * Codex logs a rollout envelope per line — `{timestamp, type, payload}` where
 * `type` ∈ session_meta | response_item | turn_context | event_msg. The timeline
 * is built from `response_item` lines plus the system prompt on `session_meta`;
 * `event_msg` is a UI/telemetry mirror consumed solely for turns/usage/
 * context-window. See the format spec.
 */

import { parseJsonl } from "../parse";
import type { ParsedSession, TimelineEvent, Turn } from "../schema";
import { estTokens, firstLine } from "../tokens";
import {
  agentMessageEvent,
  type CodexBase,
  messageEvents,
  sessionMetaEvent,
} from "./codex-context";
import { buildCodexHeader } from "./codex-header";
import type { ParseSessionArgs, SessionParser } from "./types";

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

/** Pretty-print a JSON string if it parses, else return it verbatim. */
const prettyJson = (s: string): string => {
  try {
    return JSON.stringify(JSON.parse(s), null, 2);
  } catch {
    return s;
  }
};

/** Fields shared by every timeline event built here. */
type EvBase = CodexBase;

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
    case "agent_message":
      return agentMessageEvent(payload, base);
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
  /** Indexes of model output events not yet paired with a token_count. */
  callEvents: number[];
  currentModel: string;
  currentTurnId?: string;
  /** Forked rollout still replaying the parent's history (no own output yet). */
  inherited: boolean;
  /** Last cumulative usage seen, to skip re-emitted token_counts. */
  lastTotal?: string;
  nativeWindow?: number;
}

/** Everything folded while scanning one rollout. */
interface ScanState {
  readonly counters: Map<string, number>;
  readonly cursor: Cursor;
  readonly events: TimelineEvent[];
  readonly meta: Meta;
  readonly models: Set<string>;
  readonly turns: MutTurn[];
}

/** Events the model produces in a call (as opposed to inputs fed to it). */
const isModelOutput = (ev: TimelineEvent) =>
  ev.kind === "assistant-text" ||
  ev.kind === "assistant-thinking" ||
  ev.kind === "tool-call";

/** Apply a `session_meta` payload to session metadata. */
const applySessionMeta = (
  meta: Meta,
  cursor: Cursor,
  payload: Record<string, unknown>
) => {
  if (
    meta.sessionId === undefined &&
    typeof payload.forked_from_id === "string"
  ) {
    cursor.inherited = true;
  }
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

/**
 * Register one Turn per model call from a token_count `info` block and tag the
 * call's output events with its request id. Codex re-emits token_count with an
 * unchanged cumulative total (e.g. on rate-limit updates); those are skipped.
 */
const applyTokenCount = (args: {
  readonly state: ScanState;
  readonly info: Record<string, unknown>;
  readonly index: number;
  readonly ts: string | undefined;
}) => {
  const { state, info, index, ts } = args;
  const { cursor, counters, events, turns } = state;
  const window = info.model_context_window;
  if (typeof window === "number") {
    cursor.nativeWindow = window;
  }
  const last = asObj(info.last_token_usage);
  const total =
    info.total_token_usage === undefined
      ? undefined
      : JSON.stringify(info.total_token_usage);
  if (!last || (total !== undefined && total === cursor.lastTotal)) {
    return;
  }
  if (total !== undefined) {
    cursor.lastTotal = total;
  }
  // A fork replays the parent's last token_count before its own first call.
  if (cursor.inherited) {
    return;
  }
  // `input_tokens` is the whole prompt; `cached_input_tokens` is a subset of it
  // and `reasoning_output_tokens` a subset of `output_tokens`.
  const contextTokens = num(last.input_tokens);
  const cacheReadTokens = num(last.cached_input_tokens);
  const callEvents = cursor.callEvents;
  cursor.callEvents = [];
  if (contextTokens <= 0) {
    return;
  }
  const turnId = cursor.currentTurnId ?? `tc-${index}`;
  const n = (counters.get(turnId) ?? 0) + 1;
  counters.set(turnId, n);
  const requestId = `${turnId}#${n}`;
  for (const i of callEvents) {
    const ev = events[i];
    if (ev) {
      events[i] = { ...ev, requestId };
    }
  }
  turns.push({
    requestId,
    model: cursor.currentModel,
    contextTokens,
    inputTokens: contextTokens - cacheReadTokens,
    cacheReadTokens,
    cacheCreationTokens: 0,
    outputTokens: num(last.output_tokens),
    eventIndexes: callEvents,
    ...opt("ts", ts),
  });
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
  readonly state: ScanState;
  readonly payload: Record<string, unknown>;
  readonly index: number;
  readonly ts: string | undefined;
}) => {
  const { state, payload, index, ts } = args;
  const { cursor } = state;
  const ptype = str(payload.type);
  if (ptype === "task_started") {
    // Output of an aborted call never gets a token_count; don't pair it later.
    cursor.callEvents = [];
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
      applyTokenCount({ state, info, index, ts });
    }
  }
};

/** Fold one rollout line into the running parse state. */
const applyLine = (state: ScanState, line: RawLine, index: number) => {
  const { events, models, meta, cursor } = state;
  const type = str(line.type);
  const ts = typeof line.timestamp === "string" ? line.timestamp : undefined;
  if (ts) {
    meta.startedAt ??= ts;
    meta.endedAt = ts;
  }
  const payload = asObj(line.payload) ?? {};
  const base = { index, ...opt("ts", ts) };
  const push = (ev: TimelineEvent | null) => {
    if (ev) {
      events.push(ev);
      if (isModelOutput(ev)) {
        cursor.inherited = false;
        cursor.callEvents.push(events.length - 1);
      }
    }
  };
  switch (type) {
    case "session_meta":
      applySessionMeta(meta, cursor, payload);
      push(sessionMetaEvent(payload, base));
      break;
    case "turn_context":
      applyTurnContext({ payload, cursor, models, meta });
      break;
    case "response_item":
      if (payload.type === "message") {
        for (const ev of messageEvents(payload, base)) {
          push(ev);
        }
      } else {
        push(responseEvent(payload, base));
      }
      break;
    case "event_msg":
      applyEventMsg({ state, payload, index, ts });
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
  const state: ScanState = {
    events: [],
    turns: [],
    models: new Set(),
    counters: new Map(),
    meta: {},
    cursor: { currentModel: "unknown", callEvents: [], inherited: false },
  };
  parseJsonl(text).forEach((line, index) => {
    applyLine(state, line, index);
  });
  const { events, turns, models, meta, cursor } = state;

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

/** The Codex `SessionParser`. */
export const codexParser: SessionParser = {
  agent: "codex",
  parseSession: parseCodexSession,
  buildHeader: buildCodexHeader,
};
