/** Shared history prefs: normalization, and one store every reader follows. */
import { expect, test } from "bun:test";

import {
  DEFAULT_HISTORY_PREFS,
  getHistoryPrefs,
  normalizeHistoryPrefs,
  subscribeHistoryPrefs,
  updateHistoryPrefs,
} from "../src/lib/history-prefs";
import { DEFAULT_SESSION_VIEW, normalizeView } from "../src/lib/session-view";

test("defaults to chat, transcript order, no filters", () => {
  expect(DEFAULT_HISTORY_PREFS).toEqual({
    kind: "all",
    mode: "chat",
    sort: { dir: "asc", key: "order" },
  });
});

test("non-object stored values fall back to defaults", () => {
  expect(normalizeHistoryPrefs(null)).toBe(DEFAULT_HISTORY_PREFS);
  expect(normalizeHistoryPrefs("table")).toBe(DEFAULT_HISTORY_PREFS);
});

test("a stored search query is dropped, so it cannot filter another session", () => {
  expect(normalizeHistoryPrefs({ kind: "tool-call", query: "grep" })).toEqual({
    ...DEFAULT_HISTORY_PREFS,
    kind: "tool-call",
  });
});

test("valid fields survive, bad ones reset per field", () => {
  expect(
    normalizeHistoryPrefs({
      extra: true,
      kind: 3,
      mode: "table",
      query: "grep",
      sort: "size",
    })
  ).toEqual({
    kind: "all",
    mode: "table",
    sort: { dir: "desc", key: "tokens" },
  });
  expect(normalizeHistoryPrefs({ mode: "grid", sort: "name" })).toEqual(
    DEFAULT_HISTORY_PREFS
  );
});

test("old per-session filters are dropped from the session view", () => {
  expect(
    normalizeView({
      expanded: ["event:1", 2],
      kind: "tool-call",
      query: "x",
      redacted: false,
      sort: "size",
    })
  ).toEqual({ expanded: ["event:1"], redacted: false });
  expect(normalizeView({})).toEqual(DEFAULT_SESSION_VIEW);
});

test("updates reach every subscriber and keep identity when unchanged", () => {
  let calls = 0;

  const unsubA = subscribeHistoryPrefs(() => {
    calls += 1;
  });

  const unsubB = subscribeHistoryPrefs(() => {
    calls += 1;
  });

  updateHistoryPrefs({ mode: "table", sort: { dir: "desc", key: "tokens" } });
  expect(calls).toBe(2);
  const after = getHistoryPrefs();
  expect(after.mode).toBe("table");
  expect(after.sort).toEqual({ dir: "desc", key: "tokens" });

  updateHistoryPrefs({ mode: "table", sort: { dir: "desc", key: "tokens" } });
  expect(calls).toBe(2);
  expect(getHistoryPrefs()).toBe(after);

  unsubA();
  unsubB();
  updateHistoryPrefs(DEFAULT_HISTORY_PREFS);
  expect(calls).toBe(2);
});
