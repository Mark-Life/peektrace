/** Transcript jumps: what a chip, timeline click or stats link resolves to,
 *  and where each layout finds it. */

import { describe, expect, test } from "bun:test";
import type {
  SubagentRef,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { buildChatPlan, type TranscriptRowRef } from "../src/lib/chat-lane";
import {
  isHiddenJump,
  jumpKey,
  planIndexOfTarget,
  resolveJump,
  rowIndexOfTarget,
  seedRequest,
} from "../src/lib/history-jump";

const evt = (over: Partial<TimelineEvent> & { readonly index: number }) => ({
  body: "",
  kind: "assistant-text" as const,
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const sub = (over: Partial<SubagentRef> = {}): SubagentRef => ({
  id: "agent-1",
  path: "sub.jsonl",
  peakContextTokens: 0,
  turns: 1,
  ...over,
});

/** Three turns; turn 2 holds a compaction and the spawning call. */
const EVENTS: readonly TimelineEvent[] = [
  evt({ index: 0, kind: "user-prompt", requestId: "r1" }),
  evt({ index: 1, requestId: "r1" }),
  evt({ index: 2, kind: "compaction", requestId: "r2" }),
  evt({
    index: 3,
    kind: "tool-call",
    requestId: "r2",
    toolName: "Task",
    toolUseId: "spawn",
  }),
  evt({ index: 4, kind: "tool-result", requestId: "r2", toolUseId: "spawn" }),
  evt({ index: 5, kind: "user-prompt", requestId: "r3" }),
  evt({
    index: 6,
    kind: "tool-call",
    requestId: "r3",
    toolName: "Task",
    toolUseId: "late",
  }),
];
const TURNS = [1, 1, 2, 2, 2, 3, 3];
const ROWS: readonly TranscriptRowRef[] = EVENTS.map((e, pos) => ({ e, pos }));
const planOf = (rows: readonly TranscriptRowRef[]) =>
  buildChatPlan({ crossEvtIdx: -1, rows, turns: TURNS });
const withoutTurn = (n: number) => ROWS.filter(({ pos }) => TURNS[pos] !== n);
const planOfTurn = (plan: ReturnType<typeof planOf>, turn: number) =>
  planIndexOfTarget(plan, { kind: "turn", turn });
const rowOfTurn = ({
  n,
  ordered,
  rows,
}: {
  readonly n: number;
  readonly ordered: boolean;
  readonly rows: readonly TranscriptRowRef[];
}) =>
  rowIndexOfTarget({
    ordered,
    rows,
    target: { kind: "turn", turn: n },
    turns: TURNS,
  });

const SESSION = {
  compactionTurns: [1],
  events: EVENTS,
  subagents: [sub({ toolUseId: "spawn" })],
  turnCount: 3,
};

describe("resolveJump", () => {
  test("a turn the session has stays a turn; others are no place", () => {
    expect(resolveJump(SESSION, { kind: "turn", turn: 2 })).toEqual({
      kind: "turn",
      turn: 2,
    });
    expect(resolveJump(SESSION, { kind: "turn", turn: 0 })).toBeNull();
    expect(resolveJump(SESSION, { kind: "turn", turn: 4 })).toBeNull();
  });

  test("a link's call resolves only when it is in this transcript", () => {
    expect(resolveJump(SESSION, { id: "spawn", kind: "call" })).toEqual({
      id: "spawn",
      kind: "call",
    });
    expect(resolveJump(SESSION, { id: "gone", kind: "call" })).toBeNull();
  });

  test("compaction lands on the event, or on its turn when there is none", () => {
    expect(resolveJump(SESSION, { kind: "compaction" })).toEqual({
      kind: "compaction",
    });
    const noEvent = {
      ...SESSION,
      events: EVENTS.filter((e) => e.kind !== "compaction"),
    };
    expect(resolveJump(noEvent, { kind: "compaction" })).toEqual({
      kind: "turn",
      turn: 2,
    });
    expect(
      resolveJump({ ...noEvent, compactionTurns: [] }, { kind: "compaction" })
    ).toBeNull();
  });

  test("subagent lands on its spawning call, else on the subagents list", () => {
    expect(resolveJump(SESSION, { kind: "subagent" })).toEqual({
      id: "spawn",
      kind: "call",
    });
    expect(
      resolveJump({ ...SESSION, subagents: [sub()] }, { kind: "subagent" })
    ).toEqual({ kind: "subagents" });
    // Listed in file order, not spawn order: the earliest spawn wins.
    expect(
      resolveJump(
        {
          ...SESSION,
          subagents: [
            sub({ id: "agent-b", toolUseId: "late" }),
            sub({ toolUseId: "spawn" }),
          ],
        },
        { kind: "subagent" }
      )
    ).toEqual({ id: "spawn", kind: "call" });
    // A nested agent's call lives in its parent's transcript, not this one.
    expect(
      resolveJump(
        { ...SESSION, subagents: [sub({ toolUseId: "elsewhere" })] },
        { kind: "subagent" }
      )
    ).toEqual({ kind: "subagents" });
    expect(
      resolveJump({ ...SESSION, subagents: [] }, { kind: "subagent" })
    ).toBeNull();
  });
});

test("the jump key changes with every request", () => {
  const t = { kind: "turn", turn: 3 } as const;
  expect(jumpKey(t, 1)).not.toBe(jumpKey(t, 2));
  expect(jumpKey(t, 1)).not.toBe(jumpKey({ kind: "turn", turn: 4 }, 1));
  expect(jumpKey({ id: "a", kind: "call" }, 1)).not.toBe(
    jumpKey({ id: "b", kind: "call" }, 1)
  );
});

test("a page load seeds from turn=, else call=", () => {
  expect(seedRequest({ call: null, turn: "12" })).toEqual({
    kind: "turn",
    turn: 12,
  });
  expect(seedRequest({ call: "c1", turn: "12" })).toEqual({
    kind: "turn",
    turn: 12,
  });
  expect(seedRequest({ call: "c1", turn: null })).toEqual({
    id: "c1",
    kind: "call",
  });
  for (const turn of ["0", "1.5", "x"]) {
    expect(seedRequest({ call: null, turn })).toBeNull();
  }
  expect(seedRequest({ call: null, turn: null })).toBeNull();
});

describe("chat plan", () => {
  test("a turn lands on its rule", () => {
    const plan = planOf(ROWS);
    const i = planOfTurn(plan, 2);
    expect(plan[i]).toEqual({ turn: 2, type: "turn" });
  });

  test("a filtered-away turn falls through to the next shown one", () => {
    const plan = planOf(withoutTurn(2));
    expect(plan[planOfTurn(plan, 2)]).toEqual({ turn: 3, type: "turn" });
    expect(planOfTurn(planOf(withoutTurn(3)), 3)).toBe(-1);
  });

  test("an interleaved turn lands on its first rule", () => {
    const rows = [ROWS[0], ROWS[5], ROWS[1]].filter(
      (r): r is TranscriptRowRef => r !== undefined
    );
    const plan = planOf(rows);
    expect(plan.filter((n) => n.type === "turn" && n.turn === 1)).toHaveLength(
      2
    );
    expect(planOfTurn(plan, 1)).toBe(0);
  });

  test("compaction and call targets find their nodes", () => {
    const plan = planOf(ROWS);
    const c = planIndexOfTarget(plan, { kind: "compaction" });
    expect(plan[c]?.type).toBe("band");
    const call = planIndexOfTarget(plan, { id: "spawn", kind: "call" });
    expect(plan[call]?.type).toBe("tool");
    expect(planIndexOfTarget(plan, { kind: "subagents" })).toBe(-1);
    expect(
      planIndexOfTarget(
        planOf(ROWS.filter(({ e }) => e.kind !== "compaction")),
        { kind: "compaction" }
      )
    ).toBe(-1);
  });
});

describe("table rows", () => {
  test("a turn lands on its first row", () => {
    expect(rowOfTurn({ n: 3, ordered: true, rows: ROWS })).toBe(5);
  });

  test("the later-turn fallback only applies in forward order", () => {
    const rows = withoutTurn(2);
    expect(rowOfTurn({ n: 2, ordered: true, rows })).toBe(2);
    expect(rowOfTurn({ n: 2, ordered: false, rows })).toBe(-1);
  });

  test("compaction and call targets find their rows", () => {
    expect(
      rowIndexOfTarget({
        ordered: true,
        rows: ROWS,
        target: { kind: "compaction" },
        turns: TURNS,
      })
    ).toBe(2);
    expect(
      rowIndexOfTarget({
        ordered: true,
        rows: ROWS,
        target: { id: "spawn", kind: "call" },
        turns: TURNS,
      })
    ).toBe(3);
  });
});

describe("isHiddenJump", () => {
  const jump = { key: "compaction@1", target: { kind: "compaction" } } as const;
  const base = { filtered: true, index: -1, jump, landed: null };

  test("a target the filters hide is reported until it lands", () => {
    expect(isHiddenJump(base)).toBe(true);
    expect(isHiddenJump({ ...base, landed: jump.key })).toBe(false);
    expect(isHiddenJump({ ...base, index: 2 })).toBe(false);
  });

  test("no filter, the subagents list, or no jump is never hidden", () => {
    expect(isHiddenJump({ ...base, filtered: false })).toBe(false);
    expect(
      isHiddenJump({
        ...base,
        jump: { key: "subagents@1", target: { kind: "subagents" } },
      })
    ).toBe(false);
    expect(isHiddenJump({ ...base, jump: null })).toBe(false);
  });
});
