/** Shaping `tool-call` / `tool-result` events for the AI Elements tool rows.
 *
 * Kept separate from the JSX so the interesting part — pairing a call with its
 * result, unwrapping the arg that carries the payload, deciding what outcome the
 * transcript actually records — is pure and testable. Everything here reads from
 * `TimelineEvent.body`/`preview`, which the server has already redacted, so no
 * rendering path added here can reach around the redaction toggle.
 */
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import type { ToolState } from "@workspace/ui/components/ai-elements/tool";
import type { CodeBlockLanguage } from "@workspace/ui/lib/highlighter";

/** Text shown when an event carries no body at all. */
export const EMPTY_BODY = "(no content)";

/** One labelled, highlighted slice of a tool payload. */
export interface ToolPane {
  readonly code: string;
  readonly label: string;
  readonly language: CodeBlockLanguage;
}

/** A tool call's panes plus the single line that describes it when collapsed. */
export interface ToolCallView {
  readonly panes: readonly ToolPane[];
  readonly summary: string;
}

/**
 * Tool args whose value *is* the payload, and the language it is written in.
 * Unwrapping them makes escaped newlines render as real lines (an executor
 * `code` arg, a Bash `command`) instead of one long JSON string.
 */
const PRIMARY_ARGS = [
  ["code", "typescript"],
  ["command", "bash"],
] as const satisfies readonly (readonly [string, CodeBlockLanguage])[];

const SUMMARY_CAP = 200;
const WHITESPACE = /\s+/g;

const parseJson = (s: string): unknown => {
  try {
    return JSON.parse(s);
  } catch {
    return;
  }
};

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** Join `{ type: "text", text }` blocks (standard tool_result content). */
const textFromBlocks = (v: unknown): string | null => {
  if (!Array.isArray(v)) {
    return null;
  }
  const texts = v
    .filter(
      (b): b is { text: string } =>
        isRecord(b) && b.type === "text" && typeof b.text === "string"
    )
    .map((b) => b.text);
  return texts.length > 0 ? texts.join("\n\n") : null;
};

/** Collapse a payload to a single capped line for the collapsed header. */
export const oneLine = (s: string): string => {
  const flat = s.replace(WHITESPACE, " ").trim();
  return flat.length > SUMMARY_CAP ? `${flat.slice(0, SUMMARY_CAP)}…` : flat;
};

/**
 * Split a tool call's arguments into the payload pane plus whatever else was
 * passed. The leftover args get a pane of their own, so unwrapping the payload
 * never silently hides an argument — which the raw JSON row never did either.
 */
export const callView = (e: TimelineEvent): ToolCallView => {
  const input = parseJson(e.body);
  const primary = isRecord(input)
    ? PRIMARY_ARGS.find(([key]) => typeof input[key] === "string")
    : undefined;
  if (!(primary && isRecord(input))) {
    return {
      panes: [
        { code: e.body || EMPTY_BODY, label: "Parameters", language: "json" },
      ],
      summary: e.preview,
    };
  }
  const [key, language] = primary;
  const code = String(input[key]);
  const rest = Object.entries(input).filter(([k]) => k !== key);
  const others: ToolPane[] =
    rest.length > 0
      ? [
          {
            code: JSON.stringify(Object.fromEntries(rest), null, 2),
            label: "Other parameters",
            language: "json",
          },
        ]
      : [];
  return {
    panes: [{ code, label: key, language }, ...others],
    summary: oneLine(code),
  };
};

/**
 * Shape a tool result for display: standard `{ type: "text" }` blocks join into
 * markdown, other JSON is pretty-printed, anything unparseable stays verbatim.
 */
export const resultPane = (e: TimelineEvent): Omit<ToolPane, "label"> => {
  const parsed = parseJson(e.body);
  const text = parsed === undefined ? null : textFromBlocks(parsed);
  if (text !== null) {
    return { code: text, language: "markdown" };
  }
  if (parsed !== undefined) {
    return { code: JSON.stringify(parsed, null, 2), language: "json" };
  }
  return { code: e.body || EMPTY_BODY, language: "text" };
};

/** Every tool call and result in the session, indexed by `toolUseId`. */
export interface ToolIndex {
  readonly calls: ReadonlyMap<string, TimelineEvent>;
  readonly results: ReadonlyMap<string, TimelineEvent>;
}

/** Pair calls with results so either half can name its tool and its outcome. */
export const indexTools = (a: AnalyzedSession): ToolIndex => {
  const calls = new Map<string, TimelineEvent>();
  const results = new Map<string, TimelineEvent>();
  for (const e of a.events) {
    if (!e.toolUseId) {
      continue;
    }
    if (e.kind === "tool-call") {
      calls.set(e.toolUseId, e);
    } else if (e.kind === "tool-result") {
      results.set(e.toolUseId, e);
    }
  }
  return { calls, results };
};

/** A call is `unanswered` when the transcript ends (or is cut) before its result. */
export const toolState = (e: TimelineEvent, tools: ToolIndex): ToolState => {
  if (e.kind === "tool-result") {
    return e.isError ? "error" : "completed";
  }
  const result = e.toolUseId ? tools.results.get(e.toolUseId) : undefined;
  if (!result) {
    return "unanswered";
  }
  return result.isError ? "error" : "completed";
};

/** Results carry no tool name of their own; borrow it from the paired call. */
export const toolNameOf = (e: TimelineEvent, tools: ToolIndex): string =>
  e.toolName ??
  (e.toolUseId ? tools.calls.get(e.toolUseId)?.toolName : undefined) ??
  "tool";

/** Whether an event kind gets the AI Elements tool treatment. */
export const isToolEvent = (e: TimelineEvent): boolean =>
  e.kind === "tool-call" || e.kind === "tool-result";
