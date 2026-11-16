/** Memory live refresh: a watcher bump on the memory counter settles on the
 *  next tick, a refresh of the vaults refetches `/api/memory`, and a paused
 *  view holds the bump back until resumed. */
import { afterEach, expect, test } from "bun:test";
import { Registry } from "@effect-atom/atom-react";
import { allVaultsAtom } from "../src/lib/atoms";
import {
  livePausedAtom,
  liveWaitingAtom,
  settledMemoryVersionAtom,
  watchVersionsAtom,
} from "../src/lib/watch-atoms";

const realFetch = globalThis.fetch;
const EMPTY_VAULTS = '{"projects":[],"vaults":[],"labels":[]}';

afterEach(() => {
  globalThis.fetch = realFetch;
});

/** Count fetches of each path. */
const countFetches = () => {
  const paths: string[] = [];
  globalThis.fetch = ((input: string | URL | Request) => {
    const url = input instanceof Request ? input.url : String(input);
    paths.push(new URL(url, "http://localhost").pathname);
    return Promise.resolve(
      new Response(EMPTY_VAULTS, {
        status: 200,
        headers: { "content-type": "application/json" },
      })
    );
  }) as unknown as typeof fetch;
  return paths;
};

const tick = () => new Promise((r) => setTimeout(r, 20));

/** The registry wiring `useWatchRefresh` does: a settled bump past the
 *  mounted value refreshes the vaults. */
const watch = () => {
  const registry = Registry.make();
  const settled: number[] = [];
  registry.mount(allVaultsAtom);
  registry.subscribe(
    settledMemoryVersionAtom,
    (v) => {
      settled.push(v);
      if (v > 0) {
        registry.refresh(allVaultsAtom);
      }
    },
    { immediate: true }
  );
  return { registry, settled };
};

const bump = (registry: Registry.Registry, memory: number) =>
  registry.set(watchVersionsAtom, { memory, sessions: 0 });

test("a memory bump settles once and refetches the vaults", async () => {
  const paths = countFetches();
  const { registry, settled } = watch();
  await tick();
  expect(paths).toEqual(["/api/memory"]);
  bump(registry, 1);
  await tick();
  expect(settled).toEqual([0, 1]);
  expect(paths).toEqual(["/api/memory", "/api/memory"]);
});

test("while paused a bump waits, then applies on resume", async () => {
  const paths = countFetches();
  const { registry, settled } = watch();
  registry.set(livePausedAtom, true);
  bump(registry, 2);
  await tick();
  expect(settled).toEqual([0]);
  expect(registry.get(liveWaitingAtom)).toBe(true);
  expect(paths).toEqual(["/api/memory"]);
  registry.set(livePausedAtom, false);
  await tick();
  expect(settled).toEqual([0, 2]);
  expect(paths).toEqual(["/api/memory", "/api/memory"]);
});
