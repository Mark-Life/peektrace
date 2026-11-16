import { describe, expect, test } from "bun:test";
import { type CompareOpts, exitDiff, normExit } from "./parity-serve-compare";
import type { ApiAnswer, RpcExit } from "./parity-serve-lib";
import { usageCalls, windowOpts } from "./parity-serve-stats";

const DAY_MS = 86_400_000;
const RECORDED_AT = Date.parse("2026-11-13T06:38:41.186Z");
/** Far enough that every fixture date leaves every default window. */
const LATER = RECORDED_AT + 400 * DAY_MS;

const windowAt = (now: number, days: number) => ({
  days,
  fromIso: new Date(now - days * DAY_MS).toISOString(),
  toIso: new Date(now).toISOString(),
});

const usageAt = (now: number, days: number, rows: readonly object[]) => ({
  schemaVersion: "peektrace-stats-usage/v1",
  generatedAt: now,
  by: "day",
  tz: "UTC",
  window: windowAt(now, days),
  rows,
  totals: { turns: rows.length },
});

const reportAt = (now: number, transcripts: number) => ({
  schemaVersion: "peektrace-stats/v2",
  generatedAt: now,
  window: windowAt(now, 60),
  corpus: { transcripts },
  causes: [],
});

const ok = (value: unknown): RpcExit => ({ kind: "ok", value });
const answer = (json: unknown): ApiAnswer => ({
  status: 200,
  json,
  body: new Uint8Array(),
  type: "application/json",
});

const FIXTURE_ROW = { key: "2026-10-20", turns: 216 };

/** The TS answer as the golden keeps it. */
const kept = (opts: CompareOpts, exit: RpcExit) =>
  opts.keepNormed === true && opts.norm !== undefined
    ? normExit(opts.norm)(exit)
    : exit;

describe("serve stats goldens replayed later", () => {
  test("a default-window usage case ignores fixture rows that aged out", () => {
    const defaults = usageCalls().filter(
      (u) => !("days" in (u.call.payload as object))
    );
    expect(defaults.length).toBeGreaterThan(0);
    for (const { opts } of defaults) {
      const recorded = kept(opts, ok(usageAt(RECORDED_AT, 30, [FIXTURE_ROW])));
      const later = usageAt(LATER, 30, []);
      expect(exitDiff(recorded, answer(later), opts.norm)).toEqual([]);
      // A re-recording on a later day stores the same value.
      expect(kept(opts, ok(later))).toEqual(recorded);
    }
  });

  test("a default-window case still compares the resolved window", () => {
    const recorded = ok(usageAt(RECORDED_AT, 30, []));
    const later = answer(usageAt(LATER, 31, []));
    expect(exitDiff(recorded, later, windowOpts().norm)).not.toEqual([]);
  });

  test("an explicit-days usage case compares every row", () => {
    const explicit = usageCalls().filter(
      (u) => "days" in (u.call.payload as object)
    );
    expect(explicit.length).toBeGreaterThan(0);
    for (const { opts } of explicit) {
      const recorded = ok(usageAt(RECORDED_AT, 36_500, [FIXTURE_ROW]));
      const rust = answer(usageAt(LATER, 36_500, []));
      expect(exitDiff(recorded, rust, opts.norm)).not.toEqual([]);
    }
  });

  test("the default report ignores its corpus, an explicit one does not", () => {
    const recorded = ok(reportAt(RECORDED_AT, 20));
    const later = answer(reportAt(LATER, 1));
    const byDefault = windowOpts();
    expect(exitDiff(kept(byDefault, recorded), later, byDefault.norm)).toEqual(
      []
    );
    expect(exitDiff(recorded, later, windowOpts(36_500).norm)).not.toEqual([]);
  });
});
