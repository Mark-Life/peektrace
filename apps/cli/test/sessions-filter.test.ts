import { expect, test } from "bun:test";
import { win32 } from "node:path";
import {
  byUpdatedDesc,
  matchesCwd,
  parseSince,
  resolveCwd,
  selectSessions,
} from "../src/commands/sessions-filter";

const NOW = Date.parse("2026-06-10T12:00:00.000Z");
const HOUR = 3_600_000;
const DAY = 24 * HOUR;

test("parseSince reads relative hours, days and weeks", () => {
  expect(parseSince("12h", NOW)).toBe(NOW - 12 * HOUR);
  expect(parseSince("7d", NOW)).toBe(NOW - 7 * DAY);
  expect(parseSince("2w", NOW)).toBe(NOW - 14 * DAY);
});

test("parseSince reads a date-only ISO value as UTC midnight", () => {
  expect(parseSince("2026-01-31", NOW)).toBe(
    Date.parse("2026-01-31T00:00:00Z")
  );
  expect(parseSince("2026-01-31T05:00:00+02:00", NOW)).toBe(
    Date.parse("2026-01-31T03:00:00Z")
  );
});

test("parseSince reads a zoneless date-time as UTC", () => {
  expect(parseSince("2026-04-15T10:00", NOW)).toBe(
    Date.parse("2026-04-15T10:00:00Z")
  );
  expect(parseSince("2026-04-15 10:00:30", NOW)).toBe(
    Date.parse("2026-04-15T10:00:30Z")
  );
});

test("parseSince rejects days that do not exist", () => {
  for (const bad of ["2026-02-30", "2026-04-31", "2026-13-01", "2026-02-29"]) {
    expect(parseSince(bad, NOW)).toBeUndefined();
  }
  expect(parseSince("2028-02-29", NOW)).toBe(
    Date.parse("2028-02-29T00:00:00Z")
  );
});

test("resolveCwd resolves and trims separators", () => {
  expect(resolveCwd(".")).toBe(process.cwd());
  expect(resolveCwd("/a/b/")).toBe("/a/b");
  expect(resolveCwd("/")).toBe("/");
});

test("matchesCwd: Windows paths with mixed separators", () => {
  expect(matchesCwd({ cwd: "C:/Users/demo/work" }, "C:\\Users\\demo")).toBe(
    true
  );
  expect(matchesCwd({ cwd: "C:\\Users\\demo\\work" }, "C:/Users/demo")).toBe(
    true
  );
  expect(matchesCwd({ cwd: "C:/Users/demo2" }, "C:\\Users\\demo")).toBe(false);
});

test("parseSince rejects junk and ambiguous units", () => {
  for (const bad of [
    "nope",
    "",
    "5m",
    "d",
    "-3d",
    "1.5d",
    "7",
    "12",
    "Nov 1",
    "2026",
    "1/2",
    "1 2",
    "99999999999999999999d",
  ]) {
    expect(parseSince(bad, NOW)).toBeUndefined();
  }
});

test("matchesCwd takes exact and nested paths on a / boundary", () => {
  expect(matchesCwd({ cwd: "/a/b" }, "/a/b")).toBe(true);
  expect(matchesCwd({ cwd: "/a/b/c" }, "/a/b")).toBe(true);
  expect(matchesCwd({ cwd: "/a/bc" }, "/a/b")).toBe(false);
  expect(matchesCwd({ cwd: "/a/b/" }, "/a/b")).toBe(true);
  expect(matchesCwd({ cwd: "/a/b" }, "/a/b/")).toBe(true);
  expect(matchesCwd({ cwd: "/a" }, "/a/b")).toBe(false);
});

test("matchesCwd handles Windows separators, trailing backslash and case", () => {
  const want = win32.resolve("C:\\Users\\demo\\work");
  expect(matchesCwd({ cwd: "C:\\Users\\demo\\work\\sub" }, want)).toBe(true);
  expect(matchesCwd({ cwd: "C:\\Users\\demo\\work\\" }, want)).toBe(true);
  expect(matchesCwd({ cwd: "c:\\users\\demo\\WORK" }, want)).toBe(true);
  expect(matchesCwd({ cwd: "C:\\Users\\demo\\workshop" }, want)).toBe(false);
  expect(matchesCwd({ cwd: "C:\\Users\\demo\\work" }, `${want}\\`)).toBe(true);
});

test("matchesCwd: root matches any row with a cwd, none without", () => {
  expect(matchesCwd({ cwd: "/a" }, "/")).toBe(true);
  expect(matchesCwd({}, "/")).toBe(false);
  expect(matchesCwd({}, "/a")).toBe(false);
});

test("sort is newest first, missing or unparseable last, ties by id", () => {
  const sorted = [
    { id: "d" },
    { id: "c", updatedAt: "garbage" },
    { id: "b", updatedAt: "2026-01-01T00:00:00Z" },
    { id: "a", updatedAt: "2026-02-01T00:00:00+00:00" },
    { id: "e", updatedAt: "2026-01-01T00:00:00.000Z" },
  ].sort(byUpdatedDesc);
  expect(sorted.map((h) => h.id)).toEqual(["a", "b", "e", "c", "d"]);
});

test("sort compares instants, not strings", () => {
  const sorted = [
    { id: "x", updatedAt: "2026-01-01T10:00:00+05:00" },
    { id: "y", updatedAt: "2026-01-01T06:00:00Z" },
  ].sort(byUpdatedDesc);
  expect(sorted.map((h) => h.id)).toEqual(["y", "x"]);
});

test("selectSessions applies since, cwd, sort, then limit", () => {
  const headers = [
    { id: "1", cwd: "/p", updatedAt: "2026-01-01T00:00:00Z" },
    { id: "2", cwd: "/p/q", updatedAt: "2026-03-01T00:00:00Z" },
    { id: "3", cwd: "/p", updatedAt: "2026-05-01T00:00:00Z" },
    { id: "4", cwd: "/z", updatedAt: "2026-06-01T00:00:00Z" },
    { id: "5", cwd: "/p" },
  ];
  const since = Date.parse("2026-02-01T00:00:00Z");
  const all = selectSessions(headers, { limit: 0, since, cwd: "/p" });
  expect(all.rows.map((h) => h.id)).toEqual(["3", "2"]);
  expect(all.hidden).toBe(0);
  const capped = selectSessions(headers, { limit: 1 });
  expect(capped.rows.map((h) => h.id)).toEqual(["4"]);
  expect(capped.hidden).toBe(4);
});
