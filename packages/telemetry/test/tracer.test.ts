import { describe, expect, it } from "bun:test";
import { Data, Effect } from "effect";
import type { WideEvent } from "../src/schema";
import { TelemetryStore, TelemetryStoreMemory } from "../src/store";
import { TelemetryTracerLive } from "../src/tracer";

class BoomError extends Data.TaggedError("BoomError")<{
  readonly message: string;
}> {}

const child = Effect.void.pipe(
  Effect.withSpan("child", { attributes: { id: "c1" } })
);

const withRoot = <A, E, R>(body: Effect.Effect<A, E, R>) =>
  body.pipe(
    Effect.withSpan("root", {
      attributes: { "peephole.root": true, "peephole.kind": "cli" },
    })
  );

/** Run a traced program over a fresh in-memory store and read back events. */
const runTraced = (program: Effect.Effect<unknown, unknown>) =>
  Effect.runPromise(
    Effect.gen(function* () {
      yield* program.pipe(Effect.catchAllCause(() => Effect.void));
      const store = yield* TelemetryStore;
      return yield* store.recent({ limit: 10, interestingOnly: false });
    }).pipe(
      Effect.provide(TelemetryTracerLive({ echo: false, appVersion: "test" })),
      Effect.provide(TelemetryStoreMemory)
    )
  ) as Promise<readonly WideEvent[]>;

describe("TelemetryTracer", () => {
  it("persists one success event with its child span (runSyncExit spike)", async () => {
    const events = await runTraced(withRoot(child));
    expect(events).toHaveLength(1);
    const e = events[0];
    if (!e) {
      throw new Error("expected one event");
    }
    expect(e.outcome).toBe("success");
    expect(e.name).toBe("root");
    expect(e.kind).toBe("cli");
    expect(e.spans.some((s) => s.attributes.id === "c1")).toBe(true);
    expect(e.attributes).not.toHaveProperty("peephole.root");
    expect(e.attributes).not.toHaveProperty("peephole.kind");
  });

  it("records a tagged failure as outcome=error", async () => {
    const program = withRoot(
      Effect.gen(function* () {
        yield* child;
        return yield* Effect.fail(new BoomError({ message: "kaboom" }));
      })
    );
    const events = await runTraced(program);
    expect(events).toHaveLength(1);
    const e = events[0];
    if (!e) {
      throw new Error("expected one event");
    }
    expect(e.outcome).toBe("error");
    if (e.outcome === "error") {
      expect(e.error.tag).toBe("BoomError");
      expect(e.error.message).toBe("kaboom");
    }
    expect(e.durationMs).toBeGreaterThan(0);
  });

  it("records a defect as outcome=defect", async () => {
    const program = withRoot(
      Effect.gen(function* () {
        yield* child;
        return yield* Effect.die(new Error("kaboom"));
      })
    );
    const events = await runTraced(program);
    expect(events).toHaveLength(1);
    const e = events[0];
    if (!e) {
      throw new Error("expected one event");
    }
    expect(e.outcome).toBe("defect");
    if (e.outcome === "defect") {
      expect(e.error.message.length).toBeGreaterThan(0);
    }
  });
});
