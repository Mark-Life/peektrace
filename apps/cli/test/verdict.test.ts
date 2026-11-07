import { expect, test } from "bun:test";
import {
  finalFractionOf,
  peakFractionOf,
  ROTTING_FRACTION,
  verdict,
} from "../src/verdict";

test("verdict bands switch at the dumb zone and at 0.7", () => {
  expect(verdict(0.39, 0.4)).toBe("Healthy");
  expect(verdict(0.4, 0.4)).toBe("Degrading");
  expect(verdict(ROTTING_FRACTION - 0.01, 0.4)).toBe("Degrading");
  expect(verdict(ROTTING_FRACTION, 0.4)).toBe("Rotting");
});

test("fractions are 0 for an unknown window", () => {
  const a = { contextWindow: 0, peakContextTokens: 5, finalContextTokens: 5 };
  expect(peakFractionOf(a)).toBe(0);
  expect(finalFractionOf(a)).toBe(0);
  expect(
    finalFractionOf({ ...a, contextWindow: 10, finalContextTokens: 2 })
  ).toBe(0.2);
});
