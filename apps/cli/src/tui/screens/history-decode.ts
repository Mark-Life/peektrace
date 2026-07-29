/** Pure decode helpers for the terminal session-history view.
 *
 * Mirrors the web inspector's `displayBody` / `turnNumbers` (see
 * `apps/inspector/src/routes/sessions/session-history.tsx`). Every function is
 * pure and unit-tested; the component wraps returned `content` in `<Highlighted>`
 * (which sanitizes) before render.
 */
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import type { Lang } from "../syntax";

/** Shown for `assistant-thinking` events whose body was not persisted. */
export const THINKING_NOTE =
  "Thinking content is not stored in the transcript (only a signature). Its cost is in the timeline thinking band.";

/** A decoded event body: the highlighter language + raw content (+ optional note). */
export interface DecodedBody {
  readonly content: string;
  readonly lang: Lang;
  readonly note?: string;
}

/** Parse JSON, returning `undefined` on any failure (never throws). */
const parseJson = (text: string): unknown => {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return;
  }
};

/** Narrow to a plain object (not an array, not null). */
const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * Join the `{ type: "text", text }` blocks of a standard `tool_result` content
 * array with blank lines. Returns `null` when `value` is not such an array.
 */
const textFromBlocks = (value: unknown): string | null => {
  if (!Array.isArray(value)) {
    return null;
  }
  const texts = value
    .filter(
      (b): b is { text: string } =>
        isRecord(b) && b.type === "text" && typeof b.text === "string"
    )
    .map((b) => b.text);
  return texts.length > 0 ? texts.join("\n\n") : null;
};

/**
 * Turn a raw event body into a highlightable block, unwrapping JSON-encoded
 * tool payloads (executor `code`, shell `command`, `tool_result` text) so
 * escaped newlines render as real lines. Prose/thinking fall back to `plain`.
 */
export const decodeBody = (e: TimelineEvent): DecodedBody => {
  switch (e.kind) {
    case "tool-call": {
      const input = parseJson(e.body);
      if (isRecord(input)) {
        if (typeof input.code === "string") {
          return { lang: "typescript", content: input.code };
        }
        if (typeof input.command === "string") {
          return { lang: "bash", content: input.command };
        }
      }
      return { lang: "json", content: e.body };
    }
    case "tool-result": {
      const parsed = parseJson(e.body);
      if (parsed === undefined) {
        return { lang: "plain", content: e.body };
      }
      const text = textFromBlocks(parsed);
      if (text !== null) {
        return { lang: "markdown", content: text };
      }
      return { lang: "json", content: JSON.stringify(parsed, null, 2) };
    }
    case "assistant-thinking":
      return e.body.trim() === ""
        ? { lang: "plain", content: "", note: THINKING_NOTE }
        : { lang: "plain", content: e.body };
    default:
      return { lang: "plain", content: e.body };
  }
};

/** The session's timeline with `system` events dropped (web hides these). */
export const visibleEvents = (s: AnalyzedSession): readonly TimelineEvent[] =>
  s.events.filter((e) => e.kind !== "system");

/**
 * Map each event's `index` to its 1-based turn number via a carry-forward walk:
 * an event without its own `requestId` (tool-results, thinking) inherits the
 * turn of the preceding model call.
 */
export const turnTags = (s: AnalyzedSession): Map<number, number> => {
  const reqToTurn = new Map(
    s.turns.map((t, i) => [t.requestId, i + 1] as const)
  );
  const out = new Map<number, number>();
  let cur = 0;
  for (const e of s.events) {
    if (e.requestId !== undefined && reqToTurn.has(e.requestId)) {
      cur = reqToTurn.get(e.requestId) ?? cur;
    }
    out.set(e.index, cur);
  }
  return out;
};
