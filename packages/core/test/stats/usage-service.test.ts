/** `makeUsage` input checks and the pass window it asks for. */
import { describe, expect, test } from "bun:test";
import { Clock, Effect } from "effect";
import type { StatsWindow } from "../../src/services/stats/schema";
import {
  makeUsage,
  type UsageRequest,
} from "../../src/services/stats/usage-service";

const NOW = Date.parse("2026-08-10T12:00:00.000Z");
const HOUR_MS = 3_600_000;
const NANOS_PER_MS = 1_000_000n;

const fixedClock: Clock.Clock = {
  [Clock.ClockTypeId]: Clock.ClockTypeId,
  currentTimeMillis: Effect.succeed(NOW),
  currentTimeNanos: Effect.succeed(BigInt(NOW) * NANOS_PER_MS),
  unsafeCurrentTimeMillis: () => NOW,
  unsafeCurrentTimeNanos: () => BigInt(NOW) * NANOS_PER_MS,
  sleep: () => Effect.void,
};

const runUsage = (req: UsageRequest) => {
  const windows: StatsWindow[] = [];
  const usage = makeUsage({
    pass: ({ window }) =>
      Effect.sync(() => {
        windows.push(window);
      }),
  });
  const result = Effect.runSync(
    Effect.either(usage(req)).pipe(Effect.withClock(fixedClock))
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

  test("block mode widens the pass by 5 hours only", () => {
    const { result, windows } = runUsage({ by: "block", days: 7, tz: "UTC" });
    expect(result._tag).toBe("Right");
    const from = Date.parse(windows[0]?.fromIso ?? "");
    expect(NOW - from).toBe(7 * 24 * HOUR_MS + 5 * HOUR_MS);
  });
});
