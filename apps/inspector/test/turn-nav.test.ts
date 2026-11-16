/** Where [ and ] land in both transcript layouts. */

import { describe, expect, test } from "bun:test";

import type { TimelineEvent } from "@workspace/view-core/api-types.gen";

import { buildChatPlan } from "../src/lib/chat-lane";
import { chatTurnStops, stepStop, tableTurnStops } from "../src/lib/turn-nav";

const evt = (over: Partial<TimelineEvent> = {}): TimelineEvent => ({
  body: "",
  index: 0,
  kind: "assistant-text",
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const events = [
  evt({ index: 0, kind: "user-prompt" }),
  evt({ index: 1 }),
  evt({ index: 2, kind: "user-prompt" }),
  evt({ index: 3 }),
  evt({ index: 4 }),
  evt({ index: 5, kind: "user-prompt" }),
];

const rows = events.map((e, pos) => ({ e, pos }));
const turns = [1, 1, 2, 2, 2, 3];

describe("chatTurnStops", () => {
  test("one stop per turn rule in the plan", () => {
    const plan = buildChatPlan({ crossEvtIdx: -1, rows, turns });
    const stops = chatTurnStops(plan);
    expect(stops).toHaveLength(3);

    for (const i of stops) {
      expect(plan[i]?.type).toBe("turn");
    }
  });
});

describe("tableTurnStops", () => {
  test("the first row of each turn in transcript order", () => {
    expect(
      tableTurnStops({ rows, sort: { dir: "asc", key: "order" }, turns })
    ).toEqual([0, 2, 5]);
  });

  test("descending order still has a stop per run", () => {
    const desc = [...rows].reverse();
    expect(
      tableTurnStops({ rows: desc, sort: { dir: "desc", key: "order" }, turns })
    ).toEqual([0, 1, 4]);
  });

  test("other sorts scatter turns, so there are no stops", () => {
    expect(
      tableTurnStops({ rows, sort: { dir: "desc", key: "tokens" }, turns })
    ).toEqual([]);
  });
});

describe("stepStop", () => {
  const stops = [0, 4, 9];

  test("above the list, next is the first turn and prev is nothing", () => {
    expect(stepStop({ anchor: -1, dir: "next", stops })).toBe(0);
    expect(stepStop({ anchor: -1, dir: "prev", stops })).toBeNull();
  });

  test("mid-turn moves to the neighbouring rules", () => {
    expect(stepStop({ anchor: 6, dir: "next", stops })).toBe(9);
    expect(stepStop({ anchor: 6, dir: "prev", stops })).toBe(4);
  });

  test("on a rule moves past it", () => {
    expect(stepStop({ anchor: 4, dir: "next", stops })).toBe(9);
    expect(stepStop({ anchor: 4, dir: "prev", stops })).toBe(0);
  });

  test("past the last rule there is no next", () => {
    expect(stepStop({ anchor: 12, dir: "next", stops })).toBeNull();
    expect(stepStop({ anchor: 12, dir: "prev", stops })).toBe(9);
  });
});
