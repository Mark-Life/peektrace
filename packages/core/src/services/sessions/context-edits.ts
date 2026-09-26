/** Token sizes of events across later context edits (see `ContextEdit`). The
 * budget walk counts the original size until the first edit's line, then each
 * edit's size from its own line on.
 */
import type { TimelineEvent } from "./schema";

/** Size the model saw before the edit. */
export const tokensBefore = (e: TimelineEvent) =>
  e.contextEdit?.status === "replaced"
    ? e.contextEdit.originalTokensEst
    : (e.tokensMeasured ?? e.tokensEst);

/** Size the model sees after the edit (0 once removed). */
export const tokensAfter = (e: TimelineEvent) =>
  e.contextEdit?.status === "removed" ? 0 : (e.tokensMeasured ?? e.tokensEst);

/** One point where an event's context size changes. */
export interface SizeStep {
  readonly atIndex: number;
  readonly tokensEst: number;
}

/** Each edit of the event in line order, with the size it leaves; the last is
 * the final edit. Empty when the event was never edited. */
export const sizeSteps = (e: TimelineEvent): SizeStep[] => {
  const edit = e.contextEdit;
  if (!edit) {
    return [];
  }
  return [
    ...(edit.steps ?? []).map((s) => ({
      atIndex: s.atIndex,
      tokensEst: s.tokensEst,
    })),
    { atIndex: edit.atIndex, tokensEst: tokensAfter(e) },
  ];
};

/** Size the model saw when it was sent the request at line `lineIndex`. */
export const tokensAt = (e: TimelineEvent, lineIndex: number) =>
  sizeSteps(e)
    .filter((s) => s.atIndex < lineIndex)
    .at(-1)?.tokensEst ?? tokensBefore(e);

const isVisibleKind = (e: TimelineEvent) =>
  e.kind === "assistant-text" || e.kind === "tool-call";

/** A model call's retained size from line `atIndex` on. */
export interface RequestStep {
  readonly atIndex: number;
  /** Thinking left in context (replacement thinking blocks). */
  thinking: number;
  visible: number;
}

/** Visible (text + tool call) size of one model call, and its edits. */
export interface RequestSize {
  /** Edits of this call's content in line order; the last is the final one. */
  steps: RequestStep[];
  visible: number;
}

/** Size a call's events leave after its final edit. */
const finalSize = (e: TimelineEvent) => {
  const edit = e.contextEdit;
  if (edit?.status === "replaced") {
    return {
      visible: isVisibleKind(e) ? e.tokensEst : 0,
      thinking: e.kind === "assistant-thinking" ? e.tokensEst : 0,
    };
  }
  return { visible: isVisibleKind(e) ? tokensAfter(e) : 0, thinking: 0 };
};

const addEvent = (size: RequestSize, e: TimelineEvent) => {
  const edit = e.contextEdit;
  if (edit?.status === "replaced") {
    size.visible += edit.originalTokensEst;
  } else if (isVisibleKind(e)) {
    size.visible += e.tokensEst;
  }
  if (!edit) {
    return;
  }
  const points = [
    ...(edit.steps ?? []).map((s) => ({
      atIndex: s.atIndex,
      visible: s.tokensEst,
      thinking: s.thinkingEst,
    })),
    { atIndex: edit.atIndex, ...finalSize(e) },
  ];
  if (size.steps.length === 0) {
    size.steps = points.map((p) => ({ ...p, visible: 0, thinking: 0 }));
  }
  points.forEach((p, i) => {
    const step = size.steps[i];
    if (step) {
      step.visible += p.visible;
      step.thinking += p.thinking;
    }
  });
};

/** Per request id, the visible size of its assistant output and its edits. */
export const requestSizes = (events: readonly TimelineEvent[]) => {
  const sizes = new Map<string, RequestSize>();
  for (const e of events) {
    if (!e.requestId) {
      continue;
    }
    const size = sizes.get(e.requestId) ?? { visible: 0, steps: [] };
    addEvent(size, e);
    sizes.set(e.requestId, size);
  }
  return sizes;
};
