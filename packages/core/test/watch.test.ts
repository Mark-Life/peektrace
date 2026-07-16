import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Fiber, Layer, Stream } from "effect";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "../src/services/agents";
import {
  type Invalidation,
  WatchService,
  WatchServiceLive,
} from "../src/services/watch";

const SLUG = "-Users-demo-proj";

let base = "";
let memFile = "";
let sessionFile = "";

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-watch-"));
  const memDir = join(base, SLUG, "memory");
  mkdirSync(memDir, { recursive: true });
  memFile = join(memDir, "note.md");
  writeFileSync(
    memFile,
    "---\nname: note\ndescription: seed\ntype: user\n---\nbody\n"
  );
  sessionFile = join(base, SLUG, "33333333-3333-4333-8333-333333333333.jsonl");
});

afterAll(() => {
  rmSync(base, { recursive: true, force: true });
});

/** AgentRegistry stub pointing Claude resolution at the temp projects root. */
const makeAgents = (): AgentRegistryShape => {
  const roots: AgentRoots = {
    id: "claude",
    home: base,
    projectsRoot: base,
    supported: true,
  };
  return {
    encodeSlug: (p) => p.replace(/[/.]/g, "-"),
    allowedRoots: [base],
    roots: () => roots,
    gitRoot: (cwd) => Effect.succeed(cwd),
    projectsRoot: () => Effect.succeed(base),
    sessionsGlob: () => Effect.succeed(join(base, "**", "*.jsonl")),
    memoryDir: ({ slug }) => Effect.succeed(join(base, slug, "memory")),
    listProjectSlugs: () => Effect.succeed([SLUG]),
    loadTranscript: () =>
      Effect.succeed({ text: "", sizeBytes: 0, mtimeMs: 0 }),
  };
};

const layer = WatchServiceLive.pipe(
  Layer.provide(Layer.succeed(AgentRegistry, makeAgents())),
  Layer.provide(BunFileSystem.layer)
);

const run = <A, E>(program: Effect.Effect<A, E, WatchService>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

describe("WatchService", () => {
  test(
    "emits debounced, scope-classified invalidations on real fs writes",
    () =>
      run(
        Effect.gen(function* () {
          const watch = yield* WatchService;

          // Accumulate every pushed invalidation (PubSub is hot, so subscribe
          // before any writes). We can't use `Stream.take(N)`: under CPU
          // starvation a burst of raw fs events can span more than one
          // `groupedWithin` window, so the same scope may flush more than once —
          // an exact event count is inherently racy. We assert on the *set* of
          // scopes seen instead. Only the main fiber reads `seen`, and only
          // after the collector is interrupted, so the plain array is safe.
          const seen: Invalidation[] = [];
          const collector = yield* watch.changes.pipe(
            Stream.runForEach((inv) => Effect.sync(() => seen.push(inv))),
            Effect.fork
          );

          // Let the recursive watcher + the subscription settle.
          yield* Effect.sleep("500 millis");
          const before = yield* watch.versions;

          // Burst-write the same memory file 3x. Within one debounce window
          // these coalesce to a single bump; under load they may split, so we
          // only require the memory scope to advance (monotonic; "any increase
          // => refetch") and the sessions scope to stay put.
          yield* Effect.sync(() => {
            for (let i = 0; i < 3; i++) {
              writeFileSync(
                memFile,
                `---\nname: note\ndescription: seed\ntype: user\n---\nedit ${i}\n`
              );
            }
          });
          yield* Effect.sleep("700 millis");
          const afterMemory = yield* watch.versions;

          // A new session transcript bumps only the sessions scope.
          yield* Effect.sync(() => {
            writeFileSync(sessionFile, '{"type":"user"}\n');
          });
          yield* Effect.sleep("700 millis");
          const afterSession = yield* watch.versions;

          yield* Fiber.interrupt(collector);

          // Memory writes advance memory, not sessions.
          expect(afterMemory.memory).toBeGreaterThan(before.memory);
          expect(afterMemory.sessions).toBe(before.sessions);
          // The session write advances sessions, not memory.
          expect(afterSession.sessions).toBeGreaterThan(afterMemory.sessions);
          expect(afterSession.memory).toBe(afterMemory.memory);

          // The push stream saw both scopes, and memory events carry the slug.
          const scopes = new Set(seen.map((e) => e.scope));
          expect(scopes.has("memory")).toBe(true);
          expect(scopes.has("sessions")).toBe(true);
          const memoryEvent = seen.find((e) => e.scope === "memory");
          expect(memoryEvent?.project).toBe(SLUG);
        })
      ),
    20_000
  );

  test("reports a no-op zero snapshot when the root is absent", () => {
    const missing = join(tmpdir(), "peektrace-watch-missing-xyz");
    const noopAgents: AgentRegistryShape = {
      ...makeAgents(),
      projectsRoot: () => Effect.succeed(missing),
    };
    const noopLayer = WatchServiceLive.pipe(
      Layer.provide(Layer.succeed(AgentRegistry, noopAgents)),
      Layer.provide(BunFileSystem.layer)
    );
    return Effect.runPromise(
      Effect.gen(function* () {
        const watch = yield* WatchService;
        const versions = yield* watch.versions;
        expect(versions).toEqual({ memory: 0, sessions: 0 });
      }).pipe(Effect.provide(noopLayer))
    );
  });
});
