/** Pi `context_edit` entries: append-only edits to an earlier entry's share of
 * model context. `replacement: null` drops the target; otherwise its content is
 * swapped. The latest edit per target wins; earlier ones still shape the
 * context between their lines. Raw history is left untouched.
 * Shape follows pi-coding-agent's `ContextEditEntry` (0.87+).
 */
import type { ContextEditStep, TimelineEvent } from "../schema";

type Raw = Record<string, unknown>;

/** One edit of a target entry. */
export interface PiEdit {
  /** Line index of the `context_edit` entry. */
  readonly atIndex: number;
  /** Replacement content, or null when the target is dropped. */
  readonly content: unknown;
  readonly removed: boolean;
}

const parseEdit = (line: Raw, atIndex: number): PiEdit | undefined => {
  const replacement = line.replacement;
  if (replacement === null) {
    return { atIndex, content: null, removed: true };
  }
  if (replacement && typeof replacement === "object") {
    const content = (replacement as Raw).content;
    if (typeof content === "string" || Array.isArray(content)) {
      return { atIndex, content, removed: false };
    }
  }
};

/** Map target entry id to its edits, oldest first. */
export const collectPiEdits = (lines: readonly Raw[]) => {
  const edits = new Map<string, PiEdit[]>();
  lines.forEach((line, atIndex) => {
    if (line.type !== "context_edit" || typeof line.targetId !== "string") {
      return;
    }
    const edit = parseEdit(line, atIndex);
    if (edit) {
      edits.set(line.targetId, [...(edits.get(line.targetId) ?? []), edit]);
    }
  });
  return edits;
};

/** Entry roles pi lets a context edit target (plus `custom_message` lines). */
const EDITABLE_ROLES = new Set(["user", "assistant", "toolResult"]);

/** True when pi would apply a context edit to this line. */
export const isEditable = (line: Raw) =>
  line.type === "custom_message" ||
  (line.type === "message" &&
    EDITABLE_ROLES.has(String((line.message as Raw | undefined)?.role)));

/** Pi wraps a string replacement in one text block for these roles. */
const BLOCK_ROLES = new Set(["assistant", "toolResult"]);

/** A copy of the target line with its content swapped for the replacement. */
export const replacedLine = (line: Raw, content: unknown): Raw => {
  if (line.type === "custom_message") {
    return { ...line, content };
  }
  const msg = (line.message ?? {}) as Raw;
  const normalized =
    typeof content === "string" && BLOCK_ROLES.has(String(msg.role))
      ? [{ type: "text", text: content }]
      : content;
  return { ...line, message: { ...msg, content: normalized } };
};

/** Steps for the event at position `i` of an entry: sizes live on the first. */
const stepsFor = (steps: readonly ContextEditStep[], i: number) => {
  if (steps.length === 0) {
    return {};
  }
  return {
    steps:
      i === 0
        ? steps
        : steps.map((s) => ({
            atIndex: s.atIndex,
            tokensEst: 0,
            thinkingEst: 0,
          })),
  };
};

/** Mark events dropped from context by the edit at `atIndex`. */
export const markRemoved = (args: {
  readonly events: readonly TimelineEvent[];
  readonly atIndex: number;
  readonly steps: readonly ContextEditStep[];
}): TimelineEvent[] =>
  args.events.map((e, i) => ({
    ...e,
    contextEdit: {
      status: "removed",
      atIndex: args.atIndex,
      ...stepsFor(args.steps, i),
    },
  }));

/** Mark replacement events; the first carries the original text and the
 * original visible size (thinking is costed from output tokens instead). */
export const markReplaced = (args: {
  readonly original: readonly TimelineEvent[];
  readonly replacement: readonly TimelineEvent[];
  readonly atIndex: number;
  readonly steps: readonly ContextEditStep[];
}): TimelineEvent[] => {
  const { original, replacement, atIndex, steps } = args;
  const originalText = original
    .map((e) => e.body)
    .filter(Boolean)
    .join("\n\n");
  const originalTokensEst = original
    .filter((e) => e.kind !== "assistant-thinking")
    .reduce((n, e) => n + e.tokensEst, 0);
  return replacement.map((e, i) => ({
    ...e,
    contextEdit:
      i === 0
        ? {
            status: "replaced",
            atIndex,
            originalTokensEst,
            original: originalText,
            ...stepsFor(steps, i),
          }
        : {
            status: "replaced",
            atIndex,
            originalTokensEst: 0,
            ...stepsFor(steps, i),
          },
  }));
};

/** The slice of parser state an edit rewrites. */
interface EditState {
  readonly events: TimelineEvent[];
  readonly turnsById: ReadonlyMap<string, { eventIndexes: number[] }>;
}

/** Drop turn links to events at or after `start` (they are being rebuilt). */
const unlinkFrom = (state: EditState, start: number) => {
  for (const turn of state.turnsById.values()) {
    turn.eventIndexes = turn.eventIndexes.filter((i) => i < start);
  }
};

/** Visible and thinking size of one entry's events. */
const entrySize = (events: readonly TimelineEvent[]) => ({
  tokensEst: events
    .filter((e) => e.kind !== "assistant-thinking")
    .reduce((n, e) => n + e.tokensEst, 0),
  thinkingEst: events
    .filter((e) => e.kind === "assistant-thinking")
    .reduce((n, e) => n + e.tokensEst, 0),
});

/** Events of `line` with `content` swapped in; the state is left as before. */
const probe = (args: {
  readonly state: EditState;
  readonly line: Raw;
  readonly content: unknown;
  readonly run: (line: Raw) => void;
}) => {
  const { state, line, content, run } = args;
  const start = state.events.length;
  run(replacedLine(line, content));
  const events = state.events.splice(start);
  unlinkFrom(state, start);
  return events;
};

/** Fold one line via `run`, then apply its context edits to the new events. */
export const foldWithEdit = (args: {
  readonly state: EditState;
  readonly line: Raw;
  readonly edits: readonly PiEdit[] | undefined;
  readonly run: (line: Raw) => void;
}) => {
  const { state, line, edits, run } = args;
  const edit = edits?.at(-1);
  const start = state.events.length;
  run(line);
  if (!edit) {
    return;
  }
  const original = state.events.splice(start);
  unlinkFrom(state, start);
  const steps = (edits ?? []).slice(0, -1).map((step) => ({
    atIndex: step.atIndex,
    ...(step.removed
      ? { tokensEst: 0, thinkingEst: 0 }
      : entrySize(probe({ state, line, content: step.content, run }))),
  }));
  if (!edit.removed) {
    run(replacedLine(line, edit.content));
    const replacement = state.events.splice(start);
    if (replacement.length > 0) {
      state.events.push(
        ...markReplaced({ original, replacement, atIndex: edit.atIndex, steps })
      );
      return;
    }
    unlinkFrom(state, start);
  }
  run(line);
  state.events.splice(start);
  state.events.push(
    ...markRemoved({ events: original, atIndex: edit.atIndex, steps })
  );
};
