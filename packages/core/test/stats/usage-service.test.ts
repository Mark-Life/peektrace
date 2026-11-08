/** `makeUsage` input checks and the pass window it asks for. */
import { describe, expect, test } from "bun:test";
import { Clock, Effect } from "effect";
import type { StatsWindow, UsageFact } from "../../src/services/stats/schema";
import {
  makeUsage,
  type UsageRequest,
} from "../../src/services/stats/usage-service";
import { usageFact } from "./facts";

const NOW = Date.parse("2026-08-10T12:00:00.000Z");
const HOUR_MS = 3_600_000;
const NANOS_PER_MS = 1_000_000n;

const clockAt = (now: number): Clock.Clock => ({
  [Clock.ClockTypeId]: Clock.ClockTypeId,
  currentTimeMillis: Effect.succeed(now),
  currentTimeNanos: Effect.succeed(BigInt(now) * NANOS_PER_MS),
  unsafeCurrentTimeMillis: () => now,
  unsafeCurrentTimeNanos: () => BigInt(now) * NANOS_PER_MS,
  sleep: () => Effect.void,
});

/** Runs `usage` over `facts`, feeding each pass only facts at or after its
 * window start, the way the real pass does. */
const runUsage = (
  req: UsageRequest,
  facts: readonly UsageFact[] = [],
  now = NOW
) => {
  const windows: StatsWindow[] = [];
  const usage = makeUsage({
    pass: ({ sinks, window }) =>
      Effect.sync(() => {
        windows.push(window);
        const fromMs = Date.parse(window.fromIso);
        for (const fact of facts) {
          if (fact.ts !== null && Date.parse(fact.ts) >= fromMs) {
            for (const sink of sinks) {
              sink.add(fact);
            }
          }
        }
      }),
  });
  const result = Effect.runSync(
    Effect.either(usage(req)).pipe(Effect.withClock(clockAt(now)))
  );
  return { result, windows };
};

const reasonOf = (req: UsageRequest) => {
  const { result } = runUsage(req);
  return result._tag === "Left" ? result.left.reason : null;
};

describe("usage request checks", () => {
  test.each([
    Number.NaN,
    0,
    -3,
    1.5,
    1e9,
  ])("days %p fails with a days message", (days) => {
    const reason = reasonOf({ by: "day", days, tz: "UTC" });
    expect(reason).toStartWith("days must be a whole number");
  });

  test("an unknown zone still names the zone", () => {
    expect(reasonOf({ by: "day", tz: "Mars/Olympus" })).toBe(
      "unknown time zone: Mars/Olympus"
    );
  });

  test("block mode widens the pass by one day", () => {
    const { result, windows } = runUsage({ by: "block", days: 7, tz: "UTC" });
    expect(result._tag).toBe("Right");
    expect(windows).toHaveLength(1);
    const from = Date.parse(windows[0]?.fromIso ?? "");
    expect(NOW - from).toBe(8 * DAY_MS);
  });
});

const DAY_MS = 24 * HOUR_MS;

const callAt = (ms: number, req: string) =>
  usageFact({ ts: new Date(ms).toISOString(), req });

const blocksFor = (
  req: UsageRequest,
  facts: readonly UsageFact[],
  now = NOW
) => {
  const { result, windows } = runUsage(req, facts, now);
  if (result._tag === "Left") {
    throw new Error(result.left.reason);
  }
  return {
    windows,
    blocks: result.right.rows.map((r) => [r.key, r.turns] as const),
  };
};

const iso = (ms: number) => new Date(ms).toISOString();

describe("block chains across the cutoff", () => {
  const cutoff = NOW - DAY_MS;
  const block = { by: "block", tz: "UTC" } as const;

  test("a call far before the cutoff still sets the block chain", () => {
    // a opens 06:00 and b joins it; c is 9h after 06:00 and opens 15:00.
    const facts = [
      callAt(cutoff - 5.5 * HOUR_MS, "a"),
      callAt(cutoff - HOUR_MS, "b"),
      callAt(cutoff + 3 * HOUR_MS, "c"),
    ];
    const late = iso(cutoff + 3 * HOUR_MS);
    expect(blocksFor({ ...block, days: 1 }, facts).blocks).toEqual([[late, 1]]);
    expect(blocksFor({ ...block, days: 2 }, facts).blocks).toEqual([
      [iso(cutoff - 6 * HOUR_MS), 2],
      [late, 1],
    ]);
  });

  test("a block that ended before the cutoff does not come back", () => {
    const now = Date.parse("2026-08-10T12:30:00.000Z");
    const facts = [
      callAt(Date.parse("2026-08-09T07:00:00.000Z"), "a"),
      callAt(Date.parse("2026-08-09T08:00:00.000Z"), "b"),
    ];
    expect(blocksFor({ ...block, days: 1 }, facts, now).blocks).toEqual([]);
  });

  test("steady work through the widened range reads full history", () => {
    // A call every 4 hours: blocks open 8 hours apart, one at the cutoff.
    const facts = Array.from({ length: 12 }, (_, i) =>
      callAt(cutoff - 40 * HOUR_MS + i * 4 * HOUR_MS, `r${i}`)
    );
    const { windows, blocks } = blocksFor({ ...block, days: 1 }, facts);
    expect(windows.map((w) => w.fromIso)).toEqual([
      iso(cutoff - DAY_MS),
      iso(0),
    ]);
    expect(blocks).toEqual([[iso(cutoff), 2]]);
  });
});
