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
    mode: "chat",
    sort: "order",
    kind: "all",
    query: "",
  });
});

test("non-object stored values fall back to defaults", () => {
  expect(normalizeHistoryPrefs(null)).toBe(DEFAULT_HISTORY_PREFS);
  expect(normalizeHistoryPrefs("table")).toBe(DEFAULT_HISTORY_PREFS);
});

test("valid fields survive, bad ones reset per field", () => {
  expect(
    normalizeHistoryPrefs({
      mode: "table",
      sort: "size",
      kind: 3,
      query: "grep",
      extra: true,
    })
  ).toEqual({ mode: "table", sort: "size", kind: "all", query: "grep" });
  expect(normalizeHistoryPrefs({ mode: "grid", sort: "name" })).toEqual(
    DEFAULT_HISTORY_PREFS
  );
});

test("old per-session filters are dropped from the session view", () => {
  expect(
    normalizeView({
      expanded: ["event:1", 2],
      redacted: false,
      query: "x",
      kind: "tool-call",
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

  updateHistoryPrefs({ mode: "table", sort: "size" });
  expect(calls).toBe(2);
  const after = getHistoryPrefs();
  expect(after.mode).toBe("table");
  expect(after.sort).toBe("size");

  updateHistoryPrefs({ mode: "table" });
  expect(calls).toBe(2);
  expect(getHistoryPrefs()).toBe(after);

  unsubA();
  unsubB();
  updateHistoryPrefs(DEFAULT_HISTORY_PREFS);
  expect(calls).toBe(2);
});
