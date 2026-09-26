/** Measured token sizes for context added between two model calls.
 *
 * Each call's usage gives the real prompt size. What the harness added between
 * call N and N+1 is `ctx(N+1) - ctx(N) - output(N)`. That delta goes to the
 * events added in between: all of it to a lone event, else split by estimate.
 * Steps the delta cannot describe (compaction, prompt change, context edits,
 * subagent lines, negative deltas) keep their chars/4 estimates.
 */
import { IMAGE_MARKER } from "./images";
import type { ParsedSession, TimelineEvent } from "./schema";

/** Split weight of one image, near a full-size screenshot on Claude. */
const IMAGE_WEIGHT = 1500;

const isOutputKind = (e: TimelineEvent) =>
  e.kind === "assistant-text" ||
  e.kind === "assistant-thinking" ||
  e.kind === "tool-call";

/** Events that reset or rewrite context, so no step can span them. */
const isBoundaryKind = (e: TimelineEvent) =>
  e.kind === "compaction" || e.kind === "system-prompt" || e.kind === "summary";

const countImages = (body: string) => body.split(IMAGE_MARKER).length - 1;

/** Share of a measured delta an event gets; images outweigh their marker text. */
const splitWeight = (e: TimelineEvent) =>
  e.tokensEst + IMAGE_WEIGHT * countImages(e.body);

/** Line indexes at which some context edit takes effect. */
const editLines = (events: readonly TimelineEvent[]) =>
  events.flatMap((e) =>
    e.contextEdit
      ? [
          e.contextEdit.atIndex,
          ...(e.contextEdit.steps ?? []).map((s) => s.atIndex),
        ]
      : []
  );

/** One gap between two consecutive model calls. */
interface Step {
  /** Positions in `events` of the inputs added between the calls. */
  readonly added: readonly number[];
  readonly clean: boolean;
  readonly delta: number;
}

/** Split `delta` over `added` by weight; rounding error goes to the largest. */
const splitDelta = (args: {
  readonly events: readonly TimelineEvent[];
  readonly added: readonly number[];
  readonly delta: number;
}) => {
  const { events, added, delta } = args;
  const weights = added.map((i) => (events[i] ? splitWeight(events[i]) : 0));
  const total = weights.reduce((a, b) => a + b, 0);
  if (total <= 0) {
    return;
  }
  // Largest-remainder rounding: shares stay >= 0 and sum exactly to delta.
  const exact = weights.map((w) => (delta * w) / total);
  const shares = exact.map(Math.floor);
  const left = delta - shares.reduce((a, b) => a + b, 0);
  const byRemainder = exact
    .map((x, i) => ({ i, r: x - Math.floor(x) }))
    .sort((a, b) => b.r - a.r || a.i - b.i);
  for (const { i } of byRemainder.slice(0, left)) {
    shares[i] = (shares[i] ?? 0) + 1;
  }
  return shares;
};

/** First position of each call in the event stream, in call order. */
const callStarts = (p: ParsedSession) => {
  const turnByReq = new Map(p.turns.map((t) => [t.requestId, t] as const));
  const starts: {
    readonly pos: number;
    readonly turn: ParsedSession["turns"][number];
  }[] = [];
  const seen = new Set<string>();
  p.events.forEach((e, pos) => {
    const turn = e.requestId ? turnByReq.get(e.requestId) : undefined;
    if (turn && !seen.has(turn.requestId)) {
      seen.add(turn.requestId);
      starts.push({ pos, turn });
    }
  });
  return { starts, turnByReq };
};

/** Every gap between consecutive calls, with its delta and whether it is usable. */
const collectSteps = (p: ParsedSession): Step[] => {
  const { events } = p;
  const { starts, turnByReq } = callStarts(p);
  const edits = editLines(events);
  const compactions = p.compactionIndexes;
  return starts.slice(1).map((next, k) => {
    const prev = starts[k] as (typeof starts)[number];
    const prevLine = events[prev.pos]?.index ?? 0;
    const nextLine = events[next.pos]?.index ?? 0;
    const between = events
      .slice(prev.pos + 1, next.pos)
      .map((e, i) => ({ e, pos: prev.pos + 1 + i }));
    const added = between.filter(
      ({ e }) => !(e.requestId || isOutputKind(e)) && splitWeight(e) > 0
    );
    const side = events[prev.pos]?.isSidechain === true;
    const foreign = between.some(
      ({ e }) =>
        (e.isSidechain === true) !== side ||
        isBoundaryKind(e) ||
        e.contextEdit?.status === "replaced" ||
        (e.requestId ? !turnByReq.has(e.requestId) : isOutputKind(e))
    );
    const crossesLine = (line: number) => line >= prevLine && line < nextLine;
    const delta =
      next.turn.contextTokens -
      prev.turn.contextTokens -
      prev.turn.outputTokens;
    return {
      delta,
      added: added.map(({ pos }) => pos),
      clean:
        !foreign &&
        (events[next.pos]?.isSidechain === true) === side &&
        delta >= 0 &&
        added.length > 0 &&
        !edits.some(crossesLine) &&
        !compactions.some(crossesLine),
    };
  });
};

/** Set `tokensMeasured` on every event whose size the usage data pins down. */
export const measureTokens = (p: ParsedSession): ParsedSession => {
  const measured = new Map<number, number>();
  for (const step of collectSteps(p)) {
    if (!step.clean) {
      continue;
    }
    const shares = splitDelta({
      events: p.events,
      added: step.added,
      delta: step.delta,
    });
    step.added.forEach((pos, i) => {
      const share = shares?.[i];
      if (share !== undefined) {
        measured.set(pos, share);
      }
    });
  }
  if (measured.size === 0) {
    return p;
  }
  return {
    ...p,
    events: p.events.map((e, pos) => {
      const tokensMeasured = measured.get(pos);
      return tokensMeasured === undefined ? e : { ...e, tokensMeasured };
    }),
  };
};
