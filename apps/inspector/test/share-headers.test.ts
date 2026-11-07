/** Structural sharing keeps unchanged session headers as the same objects, so
 *  memoized rows skip work when a live session reloads the list. */
import { expect, test } from "bun:test";
import { Atom, Registry, Result } from "@effect-atom/atom-react";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { sharedHeadersAtom, shareHeaders } from "../src/lib/share-headers";

const header = (
  id: string,
  over: Partial<SessionHeader> = {}
): SessionHeader => ({
  agent: "claude",
  id,
  messageCount: 1,
  path: `/corpus/${id}.jsonl`,
  project: "demo",
  sizeBytes: 100,
  updatedAt: "2026-01-01T00:00:00Z",
  ...over,
});

/** A reload decodes new objects with the same data. */
const reload = (hs: readonly SessionHeader[]) => hs.map((h) => ({ ...h }));

test("an unchanged reload returns the previous array", () => {
  const prev = [header("a"), header("b")];
  expect(shareHeaders(prev, reload(prev))).toBe(prev);
});

test("one changed header is new; the others keep their identity", () => {
  const prev = [header("a"), header("b"), header("c")];
  const next = reload(prev);
  next[1] = header("b", { messageCount: 2, updatedAt: "2026-01-01T00:01:00Z" });
  const shared = shareHeaders(prev, next);
  expect(shared).not.toBe(prev);
  expect(shared[0]).toBe(prev[0]);
  expect(shared[1]).toBe(next[1]);
  expect(shared[2]).toBe(prev[2]);
});

test("a size change without updatedAt still counts as a change", () => {
  const prev = [header("a", { updatedAt: undefined })];
  const next = [header("a", { sizeBytes: 200, updatedAt: undefined })];
  expect(shareHeaders(prev, next)[0]).toBe(next[0]);
});

test("an added optional field counts as a change", () => {
  const prev = [header("a")];
  const next = [header("a", { title: "named" })];
  expect(shareHeaders(prev, next)[0]).toBe(next[0]);
});

test("added, removed and reordered headers", () => {
  const prev = [header("a"), header("b"), header("c")];
  const added = shareHeaders(prev, [...reload(prev), header("d")]);
  expect(added).toHaveLength(4);
  expect(added.slice(0, 3)).toEqual(prev);
  expect(added[0]).toBe(prev[0]);

  const removed = shareHeaders(prev, reload([prev[0], prev[2]]));
  expect(removed).toHaveLength(2);
  expect(removed[0]).toBe(prev[0]);
  expect(removed[1]).toBe(prev[2]);

  const reordered = shareHeaders(prev, reload([prev[2], prev[0], prev[1]]));
  expect(reordered).not.toBe(prev);
  expect(reordered.map((h) => h.id)).toEqual(["c", "a", "b"]);
  expect(reordered[0]).toBe(prev[2]);
});

test("the shared atom notifies nobody on an equal refresh", () => {
  const first = [header("a"), header("b")];
  const source = Atom.make<Result.Result<readonly SessionHeader[]>>(
    Result.success(first)
  );
  const shared = sharedHeadersAtom(source);
  const registry = Registry.make();
  const seen: Result.Result<readonly SessionHeader[]>[] = [];
  registry.subscribe(shared, (r) => seen.push(r), { immediate: true });
  expect(seen).toHaveLength(1);

  registry.set(source, Result.success(first, { waiting: true }));
  registry.set(source, Result.success(reload(first)));
  expect(seen).toHaveLength(1);

  const grown = reload(first);
  grown[0] = header("a", { messageCount: 9 });
  registry.set(source, Result.success(grown));
  expect(seen).toHaveLength(2);
  const last = seen[1];
  if (!(last && Result.isSuccess(last))) {
    throw new Error("expected a success");
  }
  expect(last.value[0]).toBe(grown[0]);
  expect(last.value[1]).toBe(first[1]);
});
