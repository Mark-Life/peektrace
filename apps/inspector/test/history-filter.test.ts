/** Search and type filtering over the raw event list. */

import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import { filterRows, searchIndex } from "../src/lib/history-filter";

const evt = (over: Partial<TimelineEvent> = {}): TimelineEvent => ({
  body: "",
  index: 0,
  kind: "assistant-text",
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const EVENTS: readonly TimelineEvent[] = [
  evt({ kind: "user-prompt", preview: "Fix the Handler cache" }),
  evt({ kind: "system", preview: "handler cache warmed" }),
  evt({ kind: "tool-call", title: "Read handler.ts" }),
  evt({ body: "handler cache everywhere", preview: "unrelated" }),
];

const run = (kind: string, query: string) =>
  filterRows({ events: EVENTS, hay: searchIndex(EVENTS), kind, query });

describe("filterRows", () => {
  test("no query and all kinds keeps everything but harness system lines", () => {
    expect(run("all", "").map(({ pos }) => pos)).toEqual([0, 2, 3]);
  });

  test("matches title and preview, case-insensitively", () => {
    expect(run("all", "HANDLER").map(({ pos }) => pos)).toEqual([0, 2]);
    expect(run("all", "handler cache").map(({ pos }) => pos)).toEqual([0]);
  });

  test("does not search the body", () => {
    expect(run("all", "everywhere")).toEqual([]);
  });

  test("the kind filter narrows before the query", () => {
    expect(run("tool-call", "handler").map(({ pos }) => pos)).toEqual([2]);
    expect(run("system", "")).toEqual([]);
  });

  test("rows keep their event and their position in the full list", () => {
    const [row] = run("tool-call", "");
    expect(row?.pos).toBe(2);
    expect(row?.e).toBe(EVENTS[2] as TimelineEvent);
  });
});
