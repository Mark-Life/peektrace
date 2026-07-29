/** Unit tests for the pure session-history decode helpers. */
import { describe, expect, test } from "bun:test";
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import {
  decodeBody,
  THINKING_NOTE,
  turnTags,
  visibleEvents,
} from "../src/tui/screens/history-decode";

/** Build a `TimelineEvent` with sane defaults, overriding only what a test needs. */
const evt = (
  over: Partial<TimelineEvent> & { kind: TimelineEvent["kind"] }
): TimelineEvent => ({
  index: 0,
  title: "",
  preview: "",
  body: "",
  tokensEst: 0,
  ...over,
});

describe("decodeBody — tool-call", () => {
  test("input.code → typescript", () => {
    const d = decodeBody(
      evt({ kind: "tool-call", body: JSON.stringify({ code: "const x = 1" }) })
    );
    expect(d).toEqual({ lang: "typescript", content: "const x = 1" });
  });

  test("input.command → bash", () => {
    const d = decodeBody(
      evt({ kind: "tool-call", body: JSON.stringify({ command: "ls -la" }) })
    );
    expect(d).toEqual({ lang: "bash", content: "ls -la" });
  });

  test("no code/command → json (pretty input echoed)", () => {
    const body = JSON.stringify({ pattern: "foo" }, null, 2);
    const d = decodeBody(evt({ kind: "tool-call", body }));
    expect(d).toEqual({ lang: "json", content: body });
  });
});

describe("decodeBody — tool-result", () => {
  test("text-block array → markdown joined by blank lines", () => {
    const body = JSON.stringify([
      { type: "text", text: "line one" },
      { type: "image" },
      { type: "text", text: "line two" },
    ]);
    const d = decodeBody(evt({ kind: "tool-result", body }));
    expect(d).toEqual({ lang: "markdown", content: "line one\n\nline two" });
  });

  test("valid JSON object → pretty json", () => {
    const body = JSON.stringify({ ok: true });
    const d = decodeBody(evt({ kind: "tool-result", body }));
    expect(d).toEqual({
      lang: "json",
      content: JSON.stringify({ ok: true }, null, 2),
    });
  });

  test("non-JSON body → plain", () => {
    const d = decodeBody(evt({ kind: "tool-result", body: "not json {" }));
    expect(d).toEqual({ lang: "plain", content: "not json {" });
  });
});

describe("decodeBody — thinking / prose", () => {
  test("empty thinking → note", () => {
    const d = decodeBody(evt({ kind: "assistant-thinking", body: "  " }));
    expect(d).toEqual({ lang: "plain", content: "", note: THINKING_NOTE });
  });

  test("non-empty thinking → plain body", () => {
    const d = decodeBody(
      evt({ kind: "assistant-thinking", body: "reasoning" })
    );
    expect(d).toEqual({ lang: "plain", content: "reasoning" });
  });

  test("assistant-text → plain body", () => {
    const d = decodeBody(evt({ kind: "assistant-text", body: "# hi" }));
    expect(d).toEqual({ lang: "plain", content: "# hi" });
  });
});

/** Minimal analyzed-session stub carrying only what the walkers read. */
const session = (
  events: readonly TimelineEvent[],
  turns: readonly { requestId: string }[]
): AnalyzedSession => ({ events, turns }) as unknown as AnalyzedSession;

describe("visibleEvents", () => {
  test("drops system events", () => {
    const s = session(
      [
        evt({ index: 0, kind: "user-prompt" }),
        evt({ index: 1, kind: "system" }),
        evt({ index: 2, kind: "assistant-text" }),
      ],
      []
    );
    expect(visibleEvents(s).map((e) => e.index)).toEqual([0, 2]);
  });
});

describe("turnTags — carry-forward", () => {
  test("tool-results inherit the preceding turn", () => {
    const s = session(
      [
        evt({ index: 0, kind: "user-prompt" }),
        evt({ index: 1, kind: "assistant-text", requestId: "r1" }),
        evt({ index: 2, kind: "tool-result" }),
        evt({ index: 3, kind: "assistant-text", requestId: "r2" }),
        evt({ index: 4, kind: "tool-result" }),
      ],
      [{ requestId: "r1" }, { requestId: "r2" }]
    );
    const tags = turnTags(s);
    expect(tags.get(0)).toBe(0);
    expect(tags.get(1)).toBe(1);
    expect(tags.get(2)).toBe(1);
    expect(tags.get(3)).toBe(2);
    expect(tags.get(4)).toBe(2);
  });
});
