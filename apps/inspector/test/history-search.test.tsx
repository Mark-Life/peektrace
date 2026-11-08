/** History search: what a query matches, stepping through hits, the counter. */
import { expect, test } from "bun:test";
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { renderToStaticMarkup } from "react-dom/server";
import {
  matchLabel,
  searchIndexOf,
  stepHit,
  textRanges,
  visibleRows,
} from "../src/lib/history-search";
import { DEFAULT_HISTORY_SORT } from "../src/lib/history-sort";
import { DEFAULT_SESSION_VIEW } from "../src/lib/session-view";
import { HistorySearch } from "../src/routes/sessions/history-search";
import { SessionHistory } from "../src/routes/sessions/session-history";

const evt = (
  over: Partial<TimelineEvent> & { readonly index: number }
): TimelineEvent => ({
  body: "",
  kind: "assistant-text",
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const EVENTS = [
  evt({ body: "boot", index: 0, kind: "system", title: "boot" }),
  evt({ body: "intro", index: 1, preview: "intro", title: "intro" }),
  evt({
    body: "first line\nsecond line\nthe Zephyr valve leaks",
    index: 2,
    preview: "first line",
    title: "first line",
  }),
  evt({
    body: '{"cmd":"ls zephyr"}',
    index: 3,
    kind: "tool-call",
    preview: "ls",
    title: "Bash",
    tokensEst: 50,
  }),
  evt({ body: "done", index: 4, preview: "done", title: "done" }),
];
const INDEX = searchIndexOf(EVENTS);

const rows = (over: { kind?: string; query?: string } = {}) =>
  visibleRows({
    events: EVENTS,
    index: INDEX,
    kind: over.kind ?? "all",
    query: over.query ?? "",
    sort: DEFAULT_HISTORY_SORT,
  }).map(({ pos }) => pos);

test("matches text past the first line, ignoring case", () => {
  expect(rows({ query: "zephyr" })).toEqual([2, 3]);
  expect(rows({ query: "ZEPHYR VALVE" })).toEqual([2]);
});

test("kind and query combine", () => {
  expect(rows({ kind: "tool-call", query: "zephyr" })).toEqual([3]);
  expect(rows({ kind: "user-prompt", query: "zephyr" })).toEqual([]);
});

test("an empty query keeps every non-system event", () => {
  expect(rows()).toEqual([1, 2, 3, 4]);
});

test("the table sort still applies to the hits", () => {
  const sorted = visibleRows({
    events: EVENTS,
    index: INDEX,
    kind: "all",
    query: "zephyr",
    sort: { dir: "desc", key: "tokens" },
  }).map(({ pos }) => pos);
  expect(sorted).toEqual([3, 2]);
});

test("stepping wraps at both ends", () => {
  const hits = ["a", "b", "c"];
  expect(stepHit({ current: "a", dir: 1, hits })).toBe("b");
  expect(stepHit({ current: "c", dir: 1, hits })).toBe("a");
  expect(stepHit({ current: "a", dir: -1, hits })).toBe("c");
  expect(stepHit({ current: "b", dir: -1, hits })).toBe("a");
});

test("an unknown current starts at the first or last hit", () => {
  const hits = ["a", "b", "c"];
  expect(stepHit({ current: null, dir: 1, hits })).toBe("a");
  expect(stepHit({ current: "gone", dir: 1, hits })).toBe("a");
  expect(stepHit({ current: null, dir: -1, hits })).toBe("c");
  expect(stepHit({ current: "a", dir: 1, hits: [] })).toBeNull();
});

test("counter reads the total, then the position", () => {
  expect(matchLabel({ count: 0, current: -1 })).toBe("No matches");
  expect(matchLabel({ count: 1, current: -1 })).toBe("1 match");
  expect(matchLabel({ count: 6, current: -1 })).toBe("6 matches");
  expect(matchLabel({ count: 6, current: 0 })).toBe("1 of 6");
});

test("text ranges find every hit, without overlap", () => {
  expect(textRanges("Zephyr and zephyr", "zephyr")).toEqual([
    [0, 6],
    [11, 17],
  ]);
  expect(textRanges("aaaa", "aa")).toEqual([
    [0, 2],
    [2, 4],
  ]);
  expect(textRanges("none here", "zephyr")).toEqual([]);
  expect(textRanges("anything", "")).toEqual([]);
});

test("text whose lowercase changes length gets no ranges", () => {
  // "İ" lowercases to two code units, which would shift every offset after it.
  expect("İ".toLowerCase().length).toBe(2);
  expect(textRanges("İ zephyr", "zephyr")).toEqual([]);
});

const bar = (over: { query: string; count: number; current: number }) =>
  renderToStaticMarkup(
    <HistorySearch
      onClear={() => undefined}
      onQuery={() => undefined}
      onStep={() => undefined}
      {...over}
    />
  );

const EMPTY_COUNT = /data-testid="history-match-count"[^>]*><\/span>/;

/** The opening tag of the element with `testid`. */
const tagOf = (html: string, testid: string) =>
  html.match(new RegExp(`<[^>]*data-testid="${testid}"[^>]*>`))?.[0] ?? "";

test("an active query announces its position in a polite live region", () => {
  const html = bar({ count: 3, current: 1, query: "line" });
  expect(html).toContain('aria-live="polite"');
  expect(html).toContain('role="status"');
  expect(html).toContain('data-testid="history-match-count"');
  expect(html).toContain("2 of 3");
  expect(html).toContain('aria-label="Next match"');
  expect(tagOf(html, "history-search-next")).not.toContain('disabled=""');
});

test("no hits disables stepping", () => {
  const html = bar({ count: 0, current: -1, query: "nothing" });
  expect(html).toContain("No matches");
  expect(tagOf(html, "history-search-next")).toContain('disabled=""');
  expect(tagOf(html, "history-search-prev")).toContain('disabled=""');
});

test("an empty query shows no count and no step buttons", () => {
  const html = bar({ count: 0, current: -1, query: "" });
  expect(html).toMatch(EMPTY_COUNT);
  expect(html).not.toContain("history-search-next");
  expect(html).not.toContain("history-search-clear");
});

const session = (events: readonly TimelineEvent[]) =>
  ({
    budget: [],
    compactionIndexes: [],
    compactionTurns: [],
    contextWindow: 200_000,
    contextWindowInferred: false,
    dumbZoneCrossTurn: -1,
    dumbZoneFraction: 0.5,
    dumbZoneTurns: 0,
    events,
    finalContextTokens: 0,
    models: [],
    onDiskContextFiles: [],
    path: "s.jsonl",
    peakCacheReadTokens: 0,
    peakContextTokens: 0,
    peakTurnIndex: 0,
    provider: "claude" as const,
    sessionId: "s",
    snapshots: [],
    subagents: [],
    systemOverheadTokens: 0,
    toolCallCount: 0,
    totalOutputTokens: 0,
    turnCount: 0,
    turns: [],
    userMessageCount: 0,
  }) satisfies AnalyzedSession;

test("a fresh history view starts unfiltered", () => {
  const view = {
    isExpanded: () => false,
    setExpanded: () => undefined,
    setRedacted: () => undefined,
    state: DEFAULT_SESSION_VIEW,
    toggleExpanded: () => undefined,
  };
  const html = renderToStaticMarkup(
    <SessionHistory
      a={session([
        evt({ body: "alpha", index: 0, preview: "alpha", title: "alpha" }),
        evt({ body: "beta", index: 1, preview: "beta", title: "beta" }),
      ])}
      sid="s-fresh"
      view={view}
    />
  );
  expect(html).toContain("alpha");
  expect(html).toContain("beta");
  expect(html).toContain('value=""');
  expect(html).toContain("2 events");
});
