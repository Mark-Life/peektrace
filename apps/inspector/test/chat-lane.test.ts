/** The shape of the conversation, pinned.
 *
 * The chat layout is only as trustworthy as this plan: a call that loses its
 * result, an attachment that escapes its cluster, or a turn rule at the wrong
 * boundary all read as "the transcript says something it does not". These are
 * the cases the renderer cannot be inspected for by eye.
 */

import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import { EventKind } from "@workspace/core/services/sessions/schema";
import {
  bodyKey,
  bodyLength,
  buildChatPlan,
  type ChatNode,
  chatIdsOf,
  chatNodeKeys,
  chipLabel,
  type EventRow,
  laneOf,
  needsClamp,
  planIndexOfCall,
} from "../src/lib/chat-lane";
import { indexRowsOf } from "./history-rows";

const evt = (over: Partial<TimelineEvent> = {}): TimelineEvent => ({
  index: 0,
  kind: "assistant-text",
  title: "",
  preview: "",
  body: "",
  tokensEst: 0,
  ...over,
});

const rows = (events: readonly TimelineEvent[]) =>
  events.map((e, pos) => ({ e, pos }));

describe("laneOf", () => {
  test("every event kind lands in a lane", () => {
    const expected: Record<string, string> = {
      "user-prompt": "user",
      "assistant-text": "agent",
      "assistant-thinking": "agent",
      "tool-call": "agent",
      "tool-result": "agent",
      attachment: "center",
      "system-prompt": "center",
      system: "center",
      compaction: "center",
      summary: "center",
      meta: "center",
    };
    for (const kind of EventKind.literals) {
      expect(laneOf(evt({ kind }))).toBe(expected[kind] ?? "missing");
    }
  });

  test("a sidechain prompt is the agent talking to itself, not the human", () => {
    expect(laneOf(evt({ kind: "user-prompt", isSidechain: true }))).toBe(
      "agent"
    );
    expect(laneOf(evt({ kind: "user-prompt" }))).toBe("user");
  });

  test("a prompt the harness wrote is centred; a person's stays right", () => {
    for (const promptOrigin of ["human", "sdk", "command"] as const) {
      expect(laneOf(evt({ kind: "user-prompt", promptOrigin }))).toBe("user");
    }
    for (const promptOrigin of [
      "task-notification",
      "scheduled",
      "harness",
    ] as const) {
      expect(laneOf(evt({ kind: "user-prompt", promptOrigin }))).toBe("center");
    }
  });
});

describe("chipLabel", () => {
  test("a file attachment shows its bare name", () => {
    expect(
      chipLabel(evt({ kind: "attachment", title: "file: CLAUDE.md" }))
    ).toBe("CLAUDE.md");
    expect(
      chipLabel(evt({ kind: "attachment", title: "edited_text_file: a.ts" }))
    ).toBe("a.ts");
  });

  test("any other title passes through", () => {
    expect(
      chipLabel(evt({ kind: "attachment", title: "skill_listing (12 skills)" }))
    ).toBe("skill_listing (12 skills)");
  });

  test("no title falls back to the same badge word the table shows", () => {
    expect(
      chipLabel(evt({ kind: "attachment", title: "", attachmentType: "todo" }))
    ).toBe("todo");
  });
});

describe("needsClamp", () => {
  test("only long bodies clamp", () => {
    expect(needsClamp(evt({ body: "short" }))).toBe(false);
    expect(needsClamp(evt({ body: "x".repeat(801) }))).toBe(true);
  });
});

describe("buildChatPlan", () => {
  const plan = (events: readonly TimelineEvent[], crossEvtIdx = -1) =>
    buildChatPlan({
      crossEvtIdx,
      rows: rows(events),
      turns: events.map(() => 0),
    });

  test("a call and its result become one card", () => {
    const nodes = plan([
      evt({ kind: "tool-call", toolUseId: "t1" }),
      evt({ kind: "tool-result", toolUseId: "t1" }),
    ]);
    expect(nodes).toHaveLength(1);
    expect(nodes[0]).toMatchObject({ type: "tool" });
    const node = nodes[0];
    if (node?.type !== "tool") {
      throw new Error("expected a tool node");
    }
    expect(node.call?.pos).toBe(0);
    expect(node.result?.pos).toBe(1);
  });

  test("a result with no call in view stands alone", () => {
    const nodes = plan([evt({ kind: "tool-result", toolUseId: "t1" })]);
    const node = nodes[0];
    if (node?.type !== "tool") {
      throw new Error("expected a tool node");
    }
    expect(node.call).toBeNull();
    expect(node.result?.pos).toBe(0);
  });

  test("a call whose result was filtered out reads as unanswered", () => {
    const nodes = plan([
      evt({ kind: "tool-call", toolUseId: "t1" }),
      evt({ kind: "tool-result", toolUseId: "other" }),
    ]);
    expect(nodes).toHaveLength(2);
    const first = nodes[0];
    if (first?.type !== "tool") {
      throw new Error("expected a tool node");
    }
    expect(first.result).toBeNull();
  });

  test("adjacent context injections merge into one cluster", () => {
    const nodes = plan([
      evt({ kind: "attachment" }),
      evt({ kind: "attachment" }),
      evt({ kind: "meta" }),
      evt({ kind: "assistant-text" }),
    ]);
    expect(nodes.map((n) => n.type)).toEqual(["run", "message"]);
    const run = nodes[0];
    if (run?.type !== "run") {
      throw new Error("expected a run node");
    }
    expect(run.items).toHaveLength(3);
  });

  test("a compaction splits a cluster instead of joining it", () => {
    const nodes = plan([
      evt({ kind: "attachment" }),
      evt({ kind: "compaction" }),
      evt({ kind: "attachment" }),
    ]);
    expect(nodes.map((n) => n.type)).toEqual(["run", "band", "run"]);
  });

  test("a system prompt is a folded band, not a chip or bubble", () => {
    const nodes = plan([
      evt({ kind: "system-prompt" }),
      evt({ kind: "attachment" }),
    ]);
    expect(nodes.map((n) => n.type)).toEqual(["band", "run"]);
  });

  test("a task notification is a centred notice, not a user bubble", () => {
    const nodes = plan([
      evt({ kind: "user-prompt", promptOrigin: "human" }),
      evt({ kind: "user-prompt", promptOrigin: "task-notification" }),
      evt({ kind: "user-prompt", promptOrigin: "scheduled" }),
    ]);
    expect(nodes.map((n) => n.type)).toEqual(["message", "notice", "notice"]);
  });

  test("a harness system line folds into the chip cluster, not a reply", () => {
    const nodes = plan([
      evt({ kind: "attachment" }),
      evt({ kind: "system", title: "system: stop_hook_summary" }),
      evt({ kind: "assistant-text" }),
    ]);
    expect(nodes.map((n) => n.type)).toEqual(["run", "message"]);
  });

  test("turn rules mark changes only, and never turn 0", () => {
    const events = [
      evt({ kind: "attachment" }),
      evt({ kind: "user-prompt" }),
      evt({ kind: "assistant-text" }),
      evt({ kind: "user-prompt" }),
    ];
    const nodes = buildChatPlan({
      crossEvtIdx: -1,
      rows: rows(events),
      turns: [0, 1, 1, 2],
    });
    expect(nodes.filter((n) => n.type === "turn")).toEqual([
      { type: "turn", turn: 1 },
      { type: "turn", turn: 2 },
    ]);
  });

  test("the dumb-zone band lands before the crossing event", () => {
    const nodes = plan(
      [evt({ kind: "user-prompt" }), evt({ kind: "assistant-text" })],
      1
    );
    expect(nodes.map((n) => n.type)).toEqual([
      "message",
      "dumbzone",
      "message",
    ]);
  });
});

describe("chatIdsOf", () => {
  test("one id per rendered node, none for a folded-in result", () => {
    const ids = chatIdsOf(
      rows([
        evt({ kind: "user-prompt" }),
        evt({ kind: "tool-call", toolUseId: "t1" }),
        evt({ kind: "tool-result", toolUseId: "t1" }),
        evt({ kind: "attachment" }),
      ])
    );
    expect(ids).toEqual(["chat:0", "chat:1", "chat:3"]);
  });
});

describe("planIndexOfCall", () => {
  const events = [
    evt({ kind: "user-prompt", requestId: "r1" }),
    evt({ kind: "tool-call", requestId: "r1", toolUseId: "a" }),
    evt({ kind: "tool-result", requestId: "r1", toolUseId: "a" }),
    evt({ kind: "tool-result", requestId: "r1", toolUseId: "orphan" }),
  ];
  const plan = buildChatPlan({
    crossEvtIdx: -1,
    rows: rows(events),
    turns: [1, 1, 1, 1],
  });

  test("finds the card of a paired call", () => {
    const i = planIndexOfCall(plan, "a");
    expect(plan[i]).toMatchObject({ type: "tool", call: { pos: 1 } });
  });

  test("finds a result whose call is not shown", () => {
    const i = planIndexOfCall(plan, "orphan");
    expect(plan[i]).toMatchObject({
      type: "tool",
      call: null,
      result: { pos: 3 },
    });
  });

  test("finds a result alone when the filter hid its call", () => {
    const only = buildChatPlan({
      crossEvtIdx: -1,
      rows: rows(events).filter(({ e }) => e.kind === "tool-result"),
      turns: [1, 1, 1, 1],
    });
    const i = planIndexOfCall(only, "a");
    expect(only[i]).toMatchObject({ call: null, result: { pos: 2 } });
  });

  test("-1 for an id no node carries", () => {
    expect(planIndexOfCall(plan, "missing")).toBe(-1);
  });
});

describe("chatNodeKeys", () => {
  test("keys follow events, not list positions", () => {
    const events = [
      evt({ kind: "user-prompt", requestId: "r1" }),
      evt({ requestId: "r1" }),
      evt({ requestId: "r2" }),
    ];
    const all = rows(events);
    const turns = [1, 1, 2];
    const full = chatNodeKeys(
      buildChatPlan({ crossEvtIdx: -1, rows: all, turns })
    );
    const filtered = chatNodeKeys(
      buildChatPlan({ crossEvtIdx: -1, rows: all.slice(1), turns })
    );
    expect(full).toEqual(["turn1", "n0", "n1", "turn2", "n2"]);
    expect(filtered).toEqual(["turn1", "n1", "turn2", "n2"]);
  });

  test("a turn drawn twice gets distinct keys", () => {
    const events = [
      evt({ requestId: "r1" }),
      evt({ requestId: "r2" }),
      evt({ requestId: "r1" }),
    ];
    const keys = chatNodeKeys(
      buildChatPlan({ crossEvtIdx: -1, rows: rows(events), turns: [1, 2, 1] })
    );
    expect(new Set(keys).size).toBe(keys.length);
  });
});

describe("repeat chips", () => {
  const style = (over: Partial<TimelineEvent> = {}) =>
    evt({
      attachmentType: "output_style",
      body: "Style: concise",
      kind: "attachment",
      title: "output_style",
      ...over,
    });
  const turnsOf = (n: number) =>
    evt({ kind: "assistant-text", body: `reply ${n}` });
  const repeatsOf = (events: readonly TimelineEvent[], turns?: number[]) =>
    buildChatPlan({
      crossEvtIdx: -1,
      rows: rows(events),
      turns: turns ?? [],
    }).flatMap((n) => (n.type === "run" ? n.items.map((i) => i.repeat) : []));

  test("the same chip in later turns repeats; the first is fresh", () => {
    const events = [style(), turnsOf(1), style(), turnsOf(2), style()];
    expect(repeatsOf(events)).toEqual([false, true, true]);
  });

  test("a changed body is fresh, then the new body is the baseline", () => {
    const events = [
      style(),
      turnsOf(1),
      style({ body: "Style: verbose" }),
      turnsOf(2),
      style({ body: "Style: verbose" }),
    ];
    expect(repeatsOf(events)).toEqual([false, false, true]);
  });

  test("the same label under another attachment type is fresh", () => {
    const events = [
      style(),
      turnsOf(1),
      style({ attachmentType: "other_type" }),
    ];
    expect(repeatsOf(events)).toEqual([false, false]);
  });

  test("a sidechain chip does not fold against a main-thread one", () => {
    const events = [style(), turnsOf(1), style({ isSidechain: true })];
    expect(repeatsOf(events)).toEqual([false, false]);
  });

  test("different images with the same label never fold", () => {
    const image = () =>
      style({
        attachmentType: "image",
        body: "[image: image/png]",
        title: "image",
      });
    const events = [image(), turnsOf(1), image(), turnsOf(2), image()];
    expect(repeatsOf(events)).toEqual([false, false, false]);
  });

  test("a run keeps its items and order when some repeat", () => {
    const plan = buildChatPlan({
      crossEvtIdx: -1,
      rows: rows([style(), turnsOf(1), style(), style({ title: "other" })]),
      turns: [],
    });
    const run = plan.filter((n) => n.type === "run").at(-1);
    expect(run?.type === "run" ? run.items.map((i) => i.pos) : []).toEqual([
      2, 3,
    ]);
  });
});

describe("index rows", () => {
  const style = (body: string) =>
    evt({ attachmentType: "output_style", body, kind: "attachment" });
  const EVENTS = [
    evt({ body: "go", kind: "user-prompt" }),
    style("A"),
    evt({ body: "x".repeat(900), kind: "assistant-text" }),
    evt({ body: "{}", kind: "tool-call", toolUseId: "t" }),
    evt({ body: "out", kind: "tool-result", toolUseId: "t" }),
    style("A"),
    evt({ body: "reply" }),
    style("B"),
    evt({ body: "end" }),
    style("B"),
  ];
  const TURNS = [0, 1, 1, 1, 1, 2, 2, 2, 3, 3];
  /** A node by type and positions, never by its event objects. */
  const shapeOf = (n: ChatNode<EventRow>) => {
    if (n.type === "run") {
      return { items: n.items.map(({ pos, repeat }) => ({ pos, repeat })) };
    }
    if (n.type === "turn" || n.type === "dumbzone") {
      return n;
    }
    return { key: chatNodeKeys([n])[0], type: n.type };
  };
  const shape = (plan: readonly ChatNode<EventRow>[]) => plan.map(shapeOf);

  test("plan, ids and keys come out the same from rows as from events", () => {
    const full = buildChatPlan({
      crossEvtIdx: 4,
      rows: rows(EVENTS),
      turns: TURNS,
    });
    const index = indexRowsOf(EVENTS, TURNS);
    const fromRows = buildChatPlan({
      crossEvtIdx: 4,
      rows: index.map((e, pos) => ({ e, pos })),
      turns: TURNS,
    });
    expect(shape(fromRows)).toEqual(shape(full));
    expect(chatNodeKeys(fromRows)).toEqual(chatNodeKeys(full));
    expect(chatIdsOf(index.map((e, pos) => ({ e, pos })))).toEqual(
      chatIdsOf(rows(EVENTS))
    );
    expect(index.map(needsClamp)).toEqual(EVENTS.map(needsClamp));
    expect(index.map(chipLabel)).toEqual(EVENTS.map(chipLabel));
  });

  test("a body id and a length stand in for the body", () => {
    const [row] = indexRowsOf([evt({ body: "héllo 👋" })], [0]);
    expect(row && bodyLength(row)).toBe("héllo 👋".length);
    expect(bodyKey(evt({ body: "b" }))).toBe("b");
    expect(row && bodyKey(row)).toBe(0);
  });
});
