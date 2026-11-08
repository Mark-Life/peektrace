/** Session rail helpers: labels, ordering and the live window. */

import { describe, expect, test } from "bun:test";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { addToRange } from "react-day-picker";
import {
  activeDays,
  filtersFromParams,
  filtersToParams,
  isLive,
  LIVE_MS,
  matches,
  NO_FILTERS,
  projectKey,
  projectLabel,
  projectOptions,
  SKEW_MS,
  sortByRecent,
  stampTitle,
  visibleBranch,
} from "../src/routes/sessions/session-list-model";

const header = (over: Partial<SessionHeader> = {}): SessionHeader => ({
  id: "s-1",
  path: "/home/dev/code/app/s-1.jsonl",
  agent: "claude" as SessionHeader["agent"],
  project: "-home-dev-code-app",
  messageCount: 3,
  sizeBytes: 512,
  ...over,
});

describe("projectLabel", () => {
  test("uses the last folder of a POSIX cwd", () => {
    expect(projectLabel({ cwd: "/home/dev/code/app", project: "x" })).toBe(
      "app"
    );
  });

  test("ignores a trailing slash", () => {
    expect(projectLabel({ cwd: "/home/dev/code/app/", project: "x" })).toBe(
      "app"
    );
  });

  test("splits a Windows path", () => {
    expect(projectLabel({ cwd: "C:\\work\\tools\\cli", project: "x" })).toBe(
      "cli"
    );
  });

  test("falls back to the stored project without a cwd", () => {
    expect(projectLabel({ project: "-home-dev-code-app" })).toBe(
      "-home-dev-code-app"
    );
    expect(projectLabel({ cwd: "/", project: "root" })).toBe("root");
  });
});

describe("projectOptions", () => {
  test("keys options by cwd and labels them by folder", () => {
    const options = projectOptions([
      { cwd: "/home/dev/code/web", project: "p-web" },
      { cwd: "/home/dev/code/api", project: "p-api" },
      { cwd: "/home/dev/code/api", project: "p-api" },
    ]);
    expect(options).toEqual([
      { value: "/home/dev/code/api", label: "api" },
      { value: "/home/dev/code/web", label: "web" },
    ]);
  });

  test("falls back to the slug when cwd is missing", () => {
    expect(projectOptions([{ project: "p-web" }])).toEqual([
      { value: "p-web", label: "p-web" },
    ]);
  });

  test("merges harnesses that ran in the same folder into one option", () => {
    const options = projectOptions([
      { cwd: "/w/acme/app", project: "-w-acme-app" },
      { cwd: "/w/acme/app", project: "app" },
    ]);
    expect(options).toEqual([{ value: "/w/acme/app", label: "app" }]);
  });

  test("adds the parent folder when two folders share a name", () => {
    const options = projectOptions([
      { cwd: "/home/dev/one/app", project: "p-one" },
      { cwd: "/home/dev/two/app", project: "p-two" },
      { cwd: "/home/dev/code/web", project: "p-web" },
    ]);
    expect(options).toEqual([
      { value: "/home/dev/one/app", label: "one/app" },
      { value: "/home/dev/two/app", label: "two/app" },
      { value: "/home/dev/code/web", label: "web" },
    ]);
  });

  test("uses the full path when the parent folder still collides", () => {
    const options = projectOptions([
      { cwd: "/a/one/app", project: "p-a" },
      { cwd: "/b/one/app", project: "p-b" },
    ]);
    expect(options).toEqual([
      { value: "/a/one/app", label: "/a/one/app" },
      { value: "/b/one/app", label: "/b/one/app" },
    ]);
  });
});

describe("visibleBranch", () => {
  test("hides a detached HEAD and an empty branch", () => {
    expect(visibleBranch("HEAD")).toBeUndefined();
    expect(visibleBranch("")).toBeUndefined();
    expect(visibleBranch(undefined)).toBeUndefined();
  });

  test("keeps a named branch", () => {
    expect(visibleBranch("feature/search")).toBe("feature/search");
  });
});

describe("sortByRecent", () => {
  const ids = (hs: readonly SessionHeader[]) => hs.map((h) => h.id);

  test("orders by updatedAt over startedAt", () => {
    const old = header({
      id: "old-start",
      startedAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-03-01T00:00:00Z",
    });
    const fresh = header({
      id: "new-start",
      startedAt: "2026-02-01T00:00:00Z",
      updatedAt: "2026-02-01T00:10:00Z",
    });
    expect(ids(sortByRecent([fresh, old]))).toEqual(["old-start", "new-start"]);
  });

  test("compares instants across offset forms", () => {
    const utc = header({ id: "utc", updatedAt: "2026-05-01T10:30:00Z" });
    // 11:00+02:00 is 09:00Z, so it is older
    const offset = header({
      id: "offset",
      updatedAt: "2026-05-01T11:00:00+02:00",
    });
    expect(ids(sortByRecent([offset, utc]))).toEqual(["utc", "offset"]);
  });

  test("puts sessions without a timestamp last", () => {
    const none = header({ id: "none" });
    const bad = header({ id: "bad", updatedAt: "not a date" });
    const dated = header({ id: "dated", startedAt: "2026-05-01T00:00:00Z" });
    expect(ids(sortByRecent([none, bad, dated]))).toEqual([
      "dated",
      "bad",
      "none",
    ]);
  });

  test("does not pin a future-dated session to the top", () => {
    const now = Date.parse("2026-06-01T12:00:00Z");
    const future = header({
      id: "future",
      startedAt: "2026-01-01T00:00:00Z",
      updatedAt: "2099-01-01T00:00:00Z",
    });
    const recent = header({ id: "recent", updatedAt: "2026-06-01T11:59:00Z" });
    const lost = header({ id: "lost", updatedAt: "2099-01-01T00:00:00Z" });
    expect(ids(sortByRecent([lost, future, recent], now))).toEqual([
      "recent",
      "future",
      "lost",
    ]);
  });

  test("breaks ties by id", () => {
    const at = "2026-05-01T00:00:00Z";
    const hs = ["c", "a", "b"].map((id) => header({ id, updatedAt: at }));
    expect(ids(sortByRecent(hs))).toEqual(["a", "b", "c"]);
  });
});

describe("isLive", () => {
  const now = Date.parse("2026-06-01T12:00:00Z");
  const at = (ms: number) => new Date(ms).toISOString();

  test("is live just inside the window", () => {
    expect(isLive(header({ updatedAt: at(now - LIVE_MS + 1000) }), now)).toBe(
      true
    );
  });

  test("is not live just outside the window", () => {
    expect(isLive(header({ updatedAt: at(now - LIVE_MS - 1000) }), now)).toBe(
      false
    );
  });

  test("tolerates a small skew but not a stamp far in the future", () => {
    expect(isLive(header({ updatedAt: at(now + SKEW_MS - 1000) }), now)).toBe(
      true
    );
    expect(isLive(header({ updatedAt: at(now + 3_600_000) }), now)).toBe(false);
  });

  test("falls back to startedAt, and is never live without a time", () => {
    expect(isLive(header({ startedAt: at(now - 1000) }), now)).toBe(true);
    expect(isLive(header(), now)).toBe(false);
  });
});

describe("matches", () => {
  const h = header({
    id: "abc123",
    title: "Fix the parser",
    gitBranch: "HEAD",
    model: "model-a",
  });

  test("passes with no filters", () => {
    expect(matches(h, NO_FILTERS)).toBe(true);
  });

  test("searches title and id, case-insensitive", () => {
    expect(matches(h, { ...NO_FILTERS, query: "PARSER" })).toBe(true);
    expect(matches(h, { ...NO_FILTERS, query: "abc1" })).toBe(true);
    expect(matches(h, { ...NO_FILTERS, query: "nope" })).toBe(false);
  });

  test("filters by project key and by a HEAD branch", () => {
    expect(matches(h, { ...NO_FILTERS, project: "-home-dev-code-app" })).toBe(
      true
    );
    const withCwd = header({ cwd: "/home/dev/code/app" });
    expect(
      matches(withCwd, { ...NO_FILTERS, project: projectKey(withCwd) })
    ).toBe(true);
    expect(matches(h, { ...NO_FILTERS, project: "other" })).toBe(false);
    expect(matches(h, { ...NO_FILTERS, branch: "HEAD" })).toBe(true);
    expect(matches(h, { ...NO_FILTERS, model: "model-b" })).toBe(false);
  });
});

describe("date range", () => {
  // Local-midnight dates, as the day picker hands them over.
  const day = (d: number) => new Date(2026, 10, d);
  const spanning = header({
    startedAt: "2026-11-01T09:00:00Z",
    updatedAt: "2026-11-03T10:00:00Z",
  });

  test("lists every day from start to last activity", () => {
    expect(activeDays(spanning)).toEqual([
      "2026-11-01",
      "2026-11-02",
      "2026-11-03",
    ]);
    expect(activeDays(header({ updatedAt: "2026-11-08T10:00:00Z" }))).toEqual([
      "2026-11-08",
    ]);
    expect(activeDays(header())).toEqual([]);
  });

  test("skips a stamp that is not an ISO day instead of throwing", () => {
    expect(
      activeDays(
        header({
          startedAt: "1/5/2025 10:00",
          updatedAt: "2025-01-05T10:00:00Z",
        })
      )
    ).toEqual(["2025-01-05"]);
    expect(
      activeDays(
        header({ startedAt: "2025-01-05T10:00:00Z", updatedAt: "garbage" })
      )
    ).toEqual(["2025-01-05"]);
    expect(
      activeDays(header({ startedAt: "bad", updatedAt: "worse" }))
    ).toEqual([]);
  });

  test("matches any day the session was active on", () => {
    const on = (from: number, to?: number) =>
      matches(spanning, {
        ...NO_FILTERS,
        range: { from: day(from), to: to === undefined ? undefined : day(to) },
      });
    expect(on(1)).toBe(true);
    expect(on(2)).toBe(true);
    expect(on(3)).toBe(true);
    expect(on(4)).toBe(false);
    expect(on(4, 8)).toBe(false);
    expect(on(3, 8)).toBe(true);
  });

  test("names the shown stamp and adds the start", () => {
    expect(stampTitle(spanning)).toBe(
      "Last activity Nov 3, 10:00 · started Nov 1, 09:00"
    );
    expect(stampTitle(header({ startedAt: "2026-11-01T09:00:00Z" }))).toBe(
      "Last activity Nov 1, 09:00"
    );
    expect(stampTitle(header())).toBe("unknown time");
  });
});

describe("filters in the hash", () => {
  /** Filters encoded the way the list writes them, then read back. */
  const roundTrip = (f: Parameters<typeof filtersToParams>[0]) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(filtersToParams(f))) {
      if (value !== null) {
        params.set(key, value);
      }
    }
    return {
      params,
      back: filtersFromParams(new URLSearchParams(params.toString())),
    };
  };

  test("defaults stay out of the URL", () => {
    expect(
      Object.values(filtersToParams(NO_FILTERS)).every((v) => v === null)
    ).toBe(true);
    expect(filtersFromParams(new URLSearchParams())).toEqual(NO_FILTERS);
  });

  test("every facet survives a round trip", () => {
    const f = {
      query: "fix auth & retry",
      agent: "codex",
      project: "/work/app",
      branch: "feat/x",
      model: "m-1",
      source: "box",
      range: { from: new Date(2026, 0, 2), to: new Date(2026, 0, 9) },
    };
    const { params, back } = roundTrip(f);
    expect(params.get("q")).toBe("fix auth & retry");
    expect(params.get("from")).toBe("2026-01-02");
    expect(params.get("to")).toBe("2026-01-09");
    expect(back).toEqual(f);
  });

  test("a one-day range writes only from", () => {
    const day = new Date(2026, 2, 4);
    const { params, back } = roundTrip({
      ...NO_FILTERS,
      range: { from: day, to: day },
    });
    expect(params.get("to")).toBeNull();
    expect(back.range).toEqual({ from: day, to: day });
  });

  test("a from-only range reads back as that one day", () => {
    const from = new Date(2026, 2, 4);
    expect(
      roundTrip({ ...NO_FILTERS, range: { from, to: undefined } }).back.range
    ).toEqual({ from, to: from });
  });

  test("clicking the picked day again clears the range after a round trip", () => {
    const day = new Date(2026, 0, 5);
    const picked = roundTrip({
      ...NO_FILTERS,
      range: addToRange(day, undefined),
    }).back.range;
    expect(addToRange(day, picked)).toBeUndefined();
  });

  test("a reversed range is swapped", () => {
    expect(
      filtersFromParams(new URLSearchParams("from=2026-01-09&to=2026-01-02"))
        .range
    ).toEqual({ from: new Date(2026, 0, 2), to: new Date(2026, 0, 9) });
  });

  test("an impossible calendar day is rejected, not rolled over", () => {
    expect(
      filtersFromParams(new URLSearchParams("from=2026-02-31")).range
    ).toBeUndefined();
    expect(
      filtersFromParams(new URLSearchParams("from=2026-01-02&to=2026-02-30"))
        .range
    ).toEqual({ from: new Date(2026, 0, 2), to: new Date(2026, 0, 2) });
  });

  test("a partial patch writes only the keys it names", () => {
    expect(filtersToParams({ agent: "codex" })).toEqual({ agent: "codex" });
    expect(filtersToParams({ range: undefined })).toEqual({
      from: null,
      to: null,
    });
  });

  test("malformed dates fall back to no range", () => {
    expect(
      filtersFromParams(new URLSearchParams("from=abc")).range
    ).toBeUndefined();
    expect(
      filtersFromParams(new URLSearchParams("from=2026-13-xx")).range
    ).toBeUndefined();
    expect(
      filtersFromParams(new URLSearchParams("from=2026-13-01")).range
    ).toBeUndefined();
  });

  test("a to without a from is ignored", () => {
    expect(
      filtersFromParams(new URLSearchParams("to=2026-01-02")).range
    ).toBeUndefined();
  });

  test("an empty value reads as the default", () => {
    expect(filtersFromParams(new URLSearchParams("q=&agent=")).agent).toBe(
      NO_FILTERS.agent
    );
  });
});
