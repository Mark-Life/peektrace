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
  buildChatPlan,
  chatIdsOf,
  chipLabel,
  laneOf,
  needsClamp,
} from "../src/lib/chat-lane";

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
