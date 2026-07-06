import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import { AgentRegistryLive } from "../src/services/agents";
import { FsLive, ReadFs, type ReadFsShape, WriteFs } from "../src/services/fs";

const appLayer = FsLive.pipe(
  Layer.provide(AgentRegistryLive),
  Layer.provide(BunFileSystem.layer)
);

const run = <A, E>(program: Effect.Effect<A, E, ReadFs | WriteFs>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(appLayer)) as Effect.Effect<A, E, never>
  );

let dir = "";
beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), "peephole-fs-"));
});
afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe("atomicWrite", () => {
  test("writes content atomically into a real temp dir", () =>
    run(
      Effect.gen(function* () {
        const write = yield* WriteFs;
        const read = yield* ReadFs;
        const path = join(dir, "note.md");
        yield* write.atomicWrite({ path, content: "hello" });
        expect(readFileSync(path, "utf8")).toBe("hello");
        expect(yield* read.readText(path)).toBe("hello");
      })
    ));

  test("leaves no temp files behind", () =>
    run(
      Effect.gen(function* () {
        const write = yield* WriteFs;
        const path = join(dir, "clean.md");
        yield* write.atomicWrite({ path, content: "x" });
        const stray = readdirSync(dir).filter((n) => n.includes(".tmp-"));
        expect(stray).toHaveLength(0);
      })
    ));
});

describe("compare-and-swap", () => {
  test("succeeds when expectedMtime matches, rejects a stale mtime", () =>
    run(
      Effect.gen(function* () {
        const write = yield* WriteFs;
        const read = yield* ReadFs;
        const path = join(dir, "cas.md");
        yield* write.atomicWrite({ path, content: "v1" });
        const fresh = yield* read.stat(path);

        // Fresh CAS succeeds.
        yield* write.atomicWrite({
          path,
          content: "v2",
          expectedMtime: fresh.mtimeMs,
        });

        // Mutate the file out-of-band and bump its mtime far into the future.
        writeFileSync(path, "external");
        const future = new Date(Date.now() + 10_000);
        utimesSync(path, future, future);

        // Stale CAS (using the old mtime) must fail.
        const result = yield* Effect.either(
          write.atomicWrite({
            path,
            content: "v3",
            expectedMtime: fresh.mtimeMs,
          })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("FileChangedError");
        }
        // The conflicting write did not land.
        expect(readFileSync(path, "utf8")).toBe("external");
      })
    ));
});

describe("path containment", () => {
  test("rejects a path escaping every agent root and the temp dir", () =>
    run(
      Effect.gen(function* () {
        const write = yield* WriteFs;
        const escapePath = join(homedir(), `peephole-escape-${Date.now()}.txt`);
        const result = yield* Effect.either(
          write.atomicWrite({ path: escapePath, content: "nope" })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("PathOutsideRootError");
        }
      })
    ));
});

describe("read-only safe mode", () => {
  test("FsReadOnly omits write methods at the type level", () => {
    const check = (read: ReadFsShape) =>
      // @ts-expect-error atomicWrite is absent from the read-only surface
      read.atomicWrite;
    expect(typeof check).toBe("function");
  });
});
