/** Parse a Claude Code JSONL transcript into a normalized ParsedSession. */
import { attachmentEvent, type PromptSeen } from "./claude-attachments";
import {
  imageMarker,
  imageRef,
  isImageBlock,
  omitImageData,
  toolResultImageRefs,
} from "./images";
import { measureTokens } from "./measure";
import { claudePromptOrigin, promptLabel } from "./prompt-origin";
import type { ParsedSession, TimelineEvent, Turn } from "./schema";
import { estTokens, firstLine } from "./tokens";

/** A raw JSONL line, untyped. */
type RawLine = Record<string, unknown>;

/** Mutable turn used while building (schema `Turn` has readonly eventIndexes). */
type MutTurn = Omit<Turn, "eventIndexes"> & { eventIndexes: number[] };

const WHITESPACE = /\s+/g;
const TOOL_RESULT_SUFFIX = /_tool_result$/;

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

/** A prompt event, titled after whoever wrote it. */
const promptEvent = (args: {
  readonly o: RawLine;
  readonly index: number;
  readonly ts: string | undefined;
  readonly isSidechain: boolean;
  readonly text: string;
  readonly title: string;
}): TimelineEvent => {
  const { o, index, ts, isSidechain, text } = args;
  const origin = claudePromptOrigin(o, text);
  const label =
    origin === "human" || origin === "sdk"
      ? undefined
      : promptLabel(origin, text);
  return {
    index,
    kind: "user-prompt",
    isSidechain,
    title: label?.title ?? args.title,
    preview: label?.preview ?? firstLine(text),
    body: text,
    tokensEst: estTokens(text),
    promptOrigin: origin,
    ...opt("ts", ts),
    ...opt("isMeta", o.isMeta === true ? true : undefined),
    ...opt(
      "promptSource",
      typeof o.promptSource === "string" ? o.promptSource : undefined
    ),
  };
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

/** Position fields every event from one assistant line carries. */
interface AssistantBase {
  readonly index: number;
  readonly isSidechain: boolean;
  readonly requestId: string;
  readonly ts?: string;
}

type Block = Record<string, unknown>;

/** The `tool_use_id` a result block answers, spread-ready. */
const answers = (block: Block) =>
  opt(
    "toolUseId",
    typeof block.tool_use_id === "string" ? block.tool_use_id : undefined
  );

const advisorResultEvent = (
  base: AssistantBase,
  block: Block
): TimelineEvent => {
  const result = (block.content ?? {}) as Block;
  const redacted = result.type === "advisor_redacted_result";
  const text = redacted
    ? "(advisor response encrypted, not stored in transcript)"
    : str(result.text ?? result);
  return {
    ...base,
    kind: "tool-result",
    title: "advisor result",
    preview: firstLine(text),
    body: text,
    tokensEst: redacted ? 0 : estTokens(text),
    serverTool: true,
    ...answers(block),
  };
};

/** One web search hit as a Markdown list item. */
const searchHit = (r: Block) => {
  const title = str(r.title) || str(r.url);
  const age = typeof r.page_age === "string" ? ` · ${r.page_age}` : "";
  return `- [${title}](${str(r.url)})${age}`;
};

/** Text of content blocks, joined; anything else as JSON. */
const blockText = (content: unknown) =>
  Array.isArray(content)
    ? (content as Block[])
        .map((b) => (typeof b?.text === "string" ? b.text : str(b)))
        .join("\n\n")
    : str(content);

/** Body of a server tool result: hits, fetched text, or the error code. */
const serverResultBody = (block: Block) => {
  const content = block.content;
  const c = (content ?? {}) as Block;
  if (typeof c.error_code === "string") {
    return { text: `error: ${c.error_code}`, isError: true };
  }
  if (
    block.type !== "web_search_tool_result" &&
    block.type !== "web_fetch_tool_result"
  ) {
    return { text: blockText(content), isError: false };
  }
  if (Array.isArray(content)) {
    return {
      text: (content as Block[]).map(searchHit).join("\n"),
      isError: false,
    };
  }
  const doc = (c.content ?? {}) as Block;
  const source = (doc.source ?? {}) as Block;
  const text =
    typeof source.data === "string" && source.type === "text"
      ? source.data
      : "";
  const head = [str(c.url), typeof doc.title === "string" ? doc.title : ""]
    .filter(Boolean)
    .join(" — ");
  return { text: text ? `${head}\n\n${text}` : head, isError: false };
};

/**
 * A server tool's result (`web_search_tool_result`, `web_fetch_tool_result`,
 * …), which the API returns inside the assistant message. Search hits carry
 * their page text encrypted, so only titles and URLs are readable.
 */
const serverResultEvent = (
  base: AssistantBase,
  block: Block
): TimelineEvent => {
  const name = String(block.type).replace(TOOL_RESULT_SUFFIX, "");
  const { text, isError } = serverResultBody(block);
  return {
    ...base,
    kind: "tool-result",
    title: `${name} result${isError ? " (error)" : ""}`,
    preview: firstLine(text),
    body: text,
    tokensEst: estTokens(text),
    serverTool: true,
    ...opt("isError", isError ? true : undefined),
    ...answers(block),
  };
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
  if (isCompact) {
    state.compactionIndexes.push(index);
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

/** The thinking share of a call's output tokens, when usage reports it. */
const thinkingTokensOf = (usage: Record<string, unknown>) => {
  const details = usage.output_tokens_details as
    | Record<string, unknown>
    | undefined;
  const n = details?.thinking_tokens;
  return typeof n === "number" ? n : undefined;
};

/** Server tool counts a call reports, keeping only the ones it actually used. */
const serverToolCounts = (usage: Record<string, unknown>) => {
  const counts = (usage.server_tool_use ?? {}) as Record<string, unknown>;
  const positive = (v: unknown) =>
    typeof v === "number" && v > 0 ? v : undefined;
  return {
    ...opt("webSearchRequests", positive(counts.web_search_requests)),
    ...opt("webFetchRequests", positive(counts.web_fetch_requests)),
  };
};

/** Max of two optional counts; undefined only when both are. */
const maxOf = (a: number | undefined, b: number | undefined) => {
  if (a === undefined) {
    return b;
  }
  return b === undefined ? a : Math.max(a, b);
};

/**
 * Fold a later streamed line of the same request into its turn. Output-side
 * counts grow as the response streams, so keep the max; input and cache
 * counts are fixed per request and stay as first seen.
 */
const mergeStreamedUsage = (prev: MutTurn, usage: Record<string, unknown>) => {
  const output = usage.output_tokens;
  const tools = serverToolCounts(usage);
  return {
    ...prev,
    outputTokens: Math.max(
      prev.outputTokens,
      typeof output === "number" ? output : 0
    ),
    ...opt(
      "thinkingTokens",
      maxOf(prev.thinkingTokens, thinkingTokensOf(usage))
    ),
    ...opt(
      "webSearchRequests",
      maxOf(prev.webSearchRequests, tools.webSearchRequests)
    ),
    ...opt(
      "webFetchRequests",
      maxOf(prev.webFetchRequests, tools.webFetchRequests)
    ),
  };
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
  const { state, requestId, model, ts, countable } = args;
  const usage = args.usage as Record<string, number>;
  const inputTokens = usage.input_tokens ?? 0;
  const cacheRead = usage.cache_read_input_tokens ?? 0;
  const cacheCreate = usage.cache_creation_input_tokens ?? 0;
  const contextTokens = inputTokens + cacheRead + cacheCreate;
  const prev = state.turnsById.get(requestId);
  if (prev) {
    state.turnsById.set(requestId, mergeStreamedUsage(prev, args.usage));
    return;
  }
  if (contextTokens > 0 && countable) {
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
      ...opt("thinkingTokens", thinkingTokensOf(args.usage)),
      ...serverToolCounts(args.usage),
      ...opt("effort", args.effort),
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

  const turns = [...state.turnsById.values()];
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
  });
};
