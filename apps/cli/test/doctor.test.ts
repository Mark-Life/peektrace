/** `peephole doctor` export redaction over a real local SQLite store.
 *
 * Seeds a temp-dir `TelemetryStoreLive` with an event whose attribute carries a
 * secret, runs the real doctor command, then asserts the on-disk bundle has the
 * secret replaced by a `[REDACTED:...]` marker with structure intact. Telemetry
 * modules are imported dynamically so `PEEPHOLE_DIR` is set before the store's
 * db path is captured at module load.
 */
import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

describe("doctor export", () => {
  test("redacts secrets in the written bundle", async () => {
    const dir = mkdtempSync(join(tmpdir(), "peephole-doctor-"));
    process.env.PEEPHOLE_DIR = dir;

    const { Command } = await import("@effect/cli");
    const { BunContext } = await import("@effect/platform-bun");
    const { Effect } = await import("effect");
    const { TelemetryStore, TelemetryStoreLive } = await import(
      "@workspace/telemetry"
    );
    const { makeDoctor } = await import("../src/commands/doctor");

    const secret = `sk-ant-${"A".repeat(50)}`;
    const event = {
      id: crypto.randomUUID(),
      traceId: "trace-1",
      ts: Date.now(),
      kind: "cli",
      name: "cli",
      appVersion: "0.0.1",
      platform: "test",
      durationMs: 5,
      attributes: { argv: "sessions ls", token: secret },
      spans: [],
      outcome: "success",
    } as const;

    await Effect.runPromise(
      Effect.gen(function* () {
        const store = yield* TelemetryStore;
        yield* store.record(event);
      }).pipe(Effect.provide(TelemetryStoreLive))
    );

    const run = Command.run(makeDoctor(), {
      name: "peephole",
      version: "0.0.1",
    });
    await Effect.runPromise(
      run(["bun", "peephole"]).pipe(Effect.provide(BunContext.layer))
    );

    const path = join(dir, "peephole-report-1.json");
    const raw = readFileSync(path, "utf8");
    expect(raw).not.toContain(secret);
    expect(raw).toContain("[REDACTED:anthropic-key]");

    const bundle = JSON.parse(raw) as {
      schema: string;
      count: number;
      events: { attributes: { argv: string; token: string } }[];
    };
    expect(bundle.schema).toBe("peephole-report/v1");
    expect(bundle.count).toBe(1);
    expect(bundle.events[0]?.attributes.token).toBe("[REDACTED:anthropic-key]");
    expect(bundle.events[0]?.attributes.argv).toBe("sessions ls");
  });
});
