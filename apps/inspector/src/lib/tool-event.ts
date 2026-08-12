/** Shaping `tool-call` / `tool-result` events for the AI Elements tool rows.
 *
 * Kept separate from the JSX so the interesting part — pairing a call with its
 * result, unwrapping the arg that carries the payload, deciding what outcome the
 * transcript actually records — is pure and testable. Everything here reads from
 * `TimelineEvent.body`/`preview`, which the server has already redacted, so no
 * rendering path added here can reach around the redaction toggle.
 */
import type { LineDiff } from "@workspace/core/diff";
import { diffLines } from "@workspace/core/diff";
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import type { ToolState } from "@workspace/ui/components/ai-elements/tool";
import type { CodeBlockLanguage } from "@workspace/ui/lib/highlighter";

/** Text shown when an event carries no body at all. */
export const EMPTY_BODY = "(no content)";

/** One labelled, highlighted slice of a tool payload. */
export interface CodePane {
  readonly code: string;
  readonly label: string;
  readonly language: CodeBlockLanguage;
  readonly type: "code";
}

/** One replacement an edit tool recorded, shown as before against after. */
export interface DiffPane {
  readonly diff: LineDiff;
  readonly label: string;
  readonly language: CodeBlockLanguage;
  readonly newCode: string;
  readonly oldCode: string;
  readonly type: "diff";
}

export type ToolPane = CodePane | DiffPane;

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

/** Language for an unnamed payload arg, guessed from the file it edits. */
const BY_EXTENSION = [
  [/\.(ts|tsx|js|jsx|mts|cts)$/, "typescript"],
  [/\.json[c5]?$/, "json"],
  [/\.(md|mdx)$/, "markdown"],
  [/\.(sh|bash|zsh)$/, "bash"],
] as const satisfies readonly (readonly [RegExp, CodeBlockLanguage])[];

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

/** A string arg is a payload when it is named as one or spans several lines. */
const isPayload = ([key, value]: readonly [string, unknown]) =>
  typeof value === "string" &&
  (PRIMARY_ARGS.some(([k]) => k === key) || value.includes("\n"));

const filePathOf = (input: Record<string, unknown>) =>
  typeof input.file_path === "string" ? input.file_path : "";

/** Guess a highlight language from a file path, so a chat attachment chip opens
 *  a `.md` as markdown and a `.ts` as TypeScript instead of everything as text. */
export const languageOfPath = (path: string): CodeBlockLanguage =>
  BY_EXTENSION.find(([ext]) => ext.test(path))?.[1] ?? "text";

const languageOf = (
  key: string,
  input: Record<string, unknown>
): CodeBlockLanguage => {
  const named = PRIMARY_ARGS.find(([k]) => k === key);
  if (named) {
    return named[1];
  }
  return languageOfPath(filePathOf(input));
};

/** The args an edit tool carries its replacement in — `Edit` and its batch form. */
const EDIT_KEYS = ["old_string", "new_string", "edits"];

const replacementIn = (v: unknown) =>
  isRecord(v) &&
  typeof v.old_string === "string" &&
  typeof v.new_string === "string"
    ? { newCode: v.new_string, oldCode: v.old_string }
    : null;

/** Every replacement in a call: one for `Edit`, one per entry for its batch form. */
const replacements = (input: Record<string, unknown>) => {
  const batch = Array.isArray(input.edits)
    ? input.edits.map(replacementIn).filter((r) => r !== null)
    : [];
  const single = replacementIn(input);
  return single ? [single, ...batch] : batch;
};

const editPanes = (input: Record<string, unknown>): DiffPane[] => {
  const edits = replacements(input);
  const language = languageOf("", input);
  return edits.map((edit, i) => ({
    ...edit,
    diff: diffLines(edit.oldCode, edit.newCode),
    label: edits.length > 1 ? `Edit ${i + 1}` : "Edit",
    language,
    type: "diff" as const,
  }));
};

/** `src/lib/a.ts +3 −1` — what the edit touched, for the collapsed row. */
const editSummary = (
  panes: readonly DiffPane[],
  input: Record<string, unknown>
) => {
  const added = panes.reduce((n, p) => n + p.diff.added, 0);
  const removed = panes.reduce((n, p) => n + p.diff.removed, 0);
  const name = filePathOf(input).split("/").at(-1) ?? "";
  return `${name} +${added} −${removed}`.trim();
};

/**
 * Split a tool call's arguments into panes: a diff for every replacement an
 * edit tool recorded, then each remaining multi-line string as its own block —
 * source reads as source, not as one JSON line with `\n` spelled out. Whatever
 * is left keeps a pane of its own, so unwrapping never silently hides an
 * argument, which the raw JSON row never did either.
 */
export const callView = (e: TimelineEvent): ToolCallView => {
  const input = parseJson(e.body);
  if (!isRecord(input)) {
    return {
      panes: [
        {
          code: e.body || EMPTY_BODY,
          label: "Parameters",
          language: "json",
          type: "code",
        },
      ],
      summary: e.preview,
    };
  }
  const diffs = editPanes(input);
  const args = Object.entries(input).filter(
    ([key]) => !(diffs.length > 0 && EDIT_KEYS.includes(key))
  );
  const payloads = args.filter(isPayload);
  if (diffs.length === 0 && payloads.length === 0) {
    return {
      panes: [
        {
          code: e.body || EMPTY_BODY,
          label: "Parameters",
          language: "json",
          type: "code",
        },
      ],
      summary: e.preview,
    };
  }
  const panes: ToolPane[] = [
    ...diffs,
    ...payloads.map(([key, value]) => ({
      code: String(value),
      label: key,
      language: languageOf(key, input),
      type: "code" as const,
    })),
  ];
  const rest = args.filter((entry) => !isPayload(entry));
  if (rest.length > 0) {
    panes.push({
      code: JSON.stringify(Object.fromEntries(rest), null, 2),
      label: "Other parameters",
      language: "json",
      type: "code",
    });
  }
  const first = panes[0];
  return {
    panes,
    summary:
      diffs.length > 0
        ? editSummary(diffs, input)
        : oneLine(first?.type === "code" ? first.code : e.preview),
  };
};

/**
 * Shape a tool result for display: standard `{ type: "text" }` blocks join into
 * markdown, other JSON is pretty-printed, anything unparseable stays verbatim.
 */
export const resultPane = (
  e: TimelineEvent
): Omit<CodePane, "label" | "type"> => {
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
