/** Finding a tool call among the visible history rows. */

import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import { rowIndexOfCall } from "../src/lib/history-filter";

const evt = (over: Partial<TimelineEvent> = {}): TimelineEvent => ({
  body: "",
  index: 0,
  kind: "assistant-text",
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

describe("rowIndexOfCall", () => {
  const refs = (events: readonly TimelineEvent[]) =>
    events.map((e, pos) => ({ e, pos }));
  const paired = refs([
    evt({ kind: "user-prompt" }),
    evt({ kind: "tool-result", toolUseId: "a" }),
    evt({ kind: "tool-call", toolUseId: "a" }),
  ]);

  test("prefers the call row over its result", () => {
    expect(rowIndexOfCall(paired, "a")).toBe(2);
  });

  test("falls back to the result when the filter hides the call", () => {
    const resultOnly = refs([
      evt({ kind: "user-prompt" }),
      evt({ kind: "tool-result", toolUseId: "a" }),
    ]);
    expect(rowIndexOfCall(resultOnly, "a")).toBe(1);
  });

  test("returns -1 for an id no row has", () => {
    expect(rowIndexOfCall(paired, "missing")).toBe(-1);
  });
});
