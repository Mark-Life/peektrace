/** Guessed block heights: bigger when open, bounded by the CSS caps. */

import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { ChatNode } from "../src/lib/chat-lane";
import {
  CHAT_GAP,
  estimateChatNode,
  estimateTableRow,
} from "../src/lib/transcript-estimate";

const evt = (over: Partial<TimelineEvent> = {}): TimelineEvent => ({
  body: "",
  index: 0,
  kind: "assistant-text",
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const message = (
  body: string,
  kind: TimelineEvent["kind"] = "assistant-text"
) => ({ row: { e: evt({ body, kind }), pos: 0 }, type: "message" }) as const;

const NODES: readonly ChatNode[] = [
  message("short"),
  message("x".repeat(20_000)),
  message("deep thought", "assistant-thinking"),
  {
    call: { e: evt({ kind: "tool-call" }), pos: 1 },
    result: null,
    type: "tool",
  },
  {
    row: { e: evt({ kind: "compaction", body: "y".repeat(5000) }), pos: 2 },
    type: "band",
  },
  { items: [{ e: evt({ kind: "attachment" }), pos: 3 }], type: "run" },
  { turn: 1, type: "turn" },
  { type: "dumbzone" },
];

const size = (n: ChatNode, open: boolean) =>
  estimateChatNode({ allOpen: open, isOpen: () => open, n });

describe("estimateChatNode", () => {
  test("every node has room for itself and the gap below it", () => {
    for (const n of NODES) {
      expect(size(n, false)).toBeGreaterThan(CHAT_GAP);
    }
  });

  test("opening never makes a node shorter", () => {
    for (const n of NODES) {
      expect(size(n, true)).toBeGreaterThanOrEqual(size(n, false));
    }
  });

  test("a long collapsed bubble is capped at its clamp, opened at its scroll cap", () => {
    const long = message("x".repeat(20_000));
    expect(size(long, false)).toBeLessThan(300);
    expect(size(long, true)).toBeLessThan(600);
    expect(size(long, false)).toBeGreaterThan(size(message("short"), false));
  });

  test("folded thinking is one line whatever its length", () => {
    expect(size(message("z".repeat(9000), "assistant-thinking"), false)).toBe(
      size(message("z", "assistant-thinking"), false)
    );
  });

  test("a chip cluster opens only with everything else", () => {
    const run = NODES[5] as ChatNode;
    const closed = estimateChatNode({
      allOpen: false,
      isOpen: () => true,
      n: run,
    });
    const open = estimateChatNode({
      allOpen: true,
      isOpen: () => false,
      n: run,
    });
    expect(open).toBeGreaterThan(closed);
  });
});

describe("estimateTableRow", () => {
  test("open rows and the divider add height", () => {
    const base = estimateTableRow({ divider: false, open: false });
    expect(estimateTableRow({ divider: false, open: true })).toBeGreaterThan(
      base
    );
    expect(estimateTableRow({ divider: true, open: false })).toBeGreaterThan(
      base
    );
  });
});
