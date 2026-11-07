/** Settled versions: reloads wait for quiet, never starve under continuous
 *  writes, and hold while live updates are paused. */
import { expect, test } from "bun:test";
import { Atom, Registry } from "@effect-atom/atom-react";
import { settleDelay, settledAtom } from "../src/lib/settle";

const timing = { maxWait: 60, wait: 20 };

test("settleDelay waits for quiet after the last bump", () => {
  expect(
    settleDelay(
      { firstPendingAt: 0, lastBumpAt: 0, now: 0 },
      { maxWait: 4000, wait: 1500 }
    )
  ).toBe(1500);
});

test("settleDelay is capped by maxWait under continuous bumps", () => {
  const t = { maxWait: 4000, wait: 1500 };
  // A bump every second: the trailing deadline keeps moving, maxWait does not.
  const delays = [1000, 2000, 3000].map((now) =>
    settleDelay({ firstPendingAt: 0, lastBumpAt: now, now }, t)
  );
  expect(delays).toEqual([1500, 1500, 1000]);
  expect(
    settleDelay({ firstPendingAt: 0, lastBumpAt: 5000, now: 5000 }, t)
  ).toBe(0);
});

const setup = () => {
  const source = Atom.make(0);
  const paused = Atom.make(false);
  const settled = settledAtom({ paused, source, timing });
  const registry = Registry.make();
  const seen: number[] = [];
  registry.subscribe(settled, (v) => seen.push(v), { immediate: true });
  return { paused, registry, seen, source };
};

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

test("a burst of bumps emits once, with the latest value", async () => {
  const { registry, seen, source } = setup();
  registry.set(source, 1);
  registry.set(source, 2);
  registry.set(source, 3);
  expect(seen).toEqual([0]);
  await wait(50);
  expect(seen).toEqual([0, 3]);
});

test("continuous bumps still emit by maxWait", async () => {
  const { registry, seen, source } = setup();
  for (let v = 1; v <= 10; v++) {
    registry.set(source, v);
    await wait(10);
  }
  // 100 ms of bumps every 10 ms: a trailing-only debounce would not have fired.
  expect(seen.length).toBeGreaterThan(1);
  expect(seen[1]).toBeLessThan(10);
  await wait(50);
  expect(seen.at(-1)).toBe(10);
});

test("pause holds bumps; resume emits at once", async () => {
  const { paused, registry, seen, source } = setup();
  registry.set(paused, true);
  registry.set(source, 1);
  await wait(100);
  expect(seen).toEqual([0]);
  registry.set(paused, false);
  expect(seen).toEqual([0, 1]);
});

test("resume with nothing pending emits nothing", async () => {
  const { paused, registry, seen } = setup();
  registry.set(paused, true);
  registry.set(paused, false);
  await wait(50);
  expect(seen).toEqual([0]);
});
