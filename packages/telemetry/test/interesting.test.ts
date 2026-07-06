import { describe, expect, it } from "bun:test";
import { isInteresting, SLOW_MS } from "../src/interesting";
import type { WideEvent } from "../src/schema";

const base = {
  id: "1",
  traceId: "t",
  ts: 0,
  kind: "cli",
  name: "n",
  appVersion: "v",
  platform: "p",
  spans: [],
  attributes: {},
} as const;

describe("isInteresting", () => {
  it("success + fast → false", () => {
    const e = { ...base, durationMs: 10, outcome: "success" } as WideEvent;
    expect(isInteresting(e)).toBe(false);
  });

  it("success + exactly SLOW_MS → false (boundary is strict >)", () => {
    const e = { ...base, durationMs: SLOW_MS, outcome: "success" } as WideEvent;
    expect(isInteresting(e)).toBe(false);
  });

  it("success + slow → true", () => {
    const e = {
      ...base,
      durationMs: SLOW_MS + 1,
      outcome: "success",
    } as WideEvent;
    expect(isInteresting(e)).toBe(true);
  });

  it("error → true", () => {
    const e = {
      ...base,
      durationMs: 10,
      outcome: "error",
      error: { tag: "E", message: "m" },
    } as WideEvent;
    expect(isInteresting(e)).toBe(true);
  });

  it("defect → true", () => {
    const e = {
      ...base,
      durationMs: 10,
      outcome: "defect",
      error: { message: "m" },
    } as WideEvent;
    expect(isInteresting(e)).toBe(true);
  });
});
