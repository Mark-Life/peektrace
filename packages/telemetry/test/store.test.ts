import { describe, expect, it } from "bun:test";
import { SqlClient } from "@effect/sql";
import { SqliteClient } from "@effect/sql-sqlite-bun";
import { Effect, Layer } from "effect";
import type { WideEvent } from "../src/schema";
import { make, TelemetryStore, TelemetryStoreMemory } from "../src/store";

const successEvent: WideEvent = {
  id: "evt-1",
  traceId: "trace-1",
  ts: 1_700_000_000_000,
  kind: "cli",
  name: "sessions ls",
  appVersion: "0.0.1",
  platform: "test",
  durationMs: 12,
  attributes: { argv: "sessions ls" },
  spans: [{ name: "fs.read", durationMs: 3, attributes: { path: "/x" } }],
  outcome: "success",
};

const errorEvent: WideEvent = {
  id: "evt-2",
  traceId: "trace-2",
  ts: 1_700_000_000_001,
  kind: "cli",
  name: "boom",
  appVersion: "0.0.1",
  platform: "test",
  durationMs: 5,
  attributes: {},
  spans: [],
  outcome: "error",
  error: { tag: "BoomError", message: "kaboom", fields: {} },
};

const RETENTION_DAYS = 30;
const MS_PER_DAY = 86_400_000;

describe("TelemetryStore", () => {
  it("recent on an empty store returns []", async () => {
    const out = await Effect.runPromise(
      Effect.gen(function* () {
        const store = yield* TelemetryStore;
        return yield* store.recent({ limit: 10, interestingOnly: false });
      }).pipe(Effect.provide(TelemetryStoreMemory))
    );
    expect(out).toEqual([]);
  });

  it("purges rows older than the retention window at init", async () => {
    const clientLayer = SqliteClient.layer({ filename: ":memory:" });
    const staleTs = Date.now() - (RETENTION_DAYS + 1) * MS_PER_DAY;
    const freshTs = Date.now();
    const insertRow = (sql: SqlClient.SqlClient, id: string, ts: number) =>
      sql`INSERT INTO events ${sql.insert({
        id,
        ts,
        kind: "cli",
        name: "n",
        outcome: "success",
        error_tag: null,
        duration_ms: 1,
        interesting: 0,
        data: "{}",
      })}`;
    const ids = await Effect.runPromise(
      Effect.gen(function* () {
        const sql = yield* SqlClient.SqlClient;
        yield* make; // create table (purge is a no-op on the empty db)
        yield* insertRow(sql, "stale", staleTs);
        yield* insertRow(sql, "fresh", freshTs);
        yield* make; // re-init runs the retention purge over the seeded rows
        const rows = yield* sql<{
          id: string;
        }>`SELECT id FROM events ORDER BY id`;
        return rows.map((r) => r.id);
      }).pipe(Effect.provide(clientLayer))
    );
    expect(ids).toEqual(["fresh"]);
  });

  it("record then recent round-trips the event", async () => {
    const out = await Effect.runPromise(
      Effect.gen(function* () {
        const store = yield* TelemetryStore;
        yield* store.record(successEvent);
        return yield* store.recent({ limit: 10, interestingOnly: false });
      }).pipe(Effect.provide(TelemetryStoreMemory))
    );
    expect(out).toEqual([successEvent]);
  });

  it("interestingOnly filters to errors/slow events", async () => {
    const out = await Effect.runPromise(
      Effect.gen(function* () {
        const store = yield* TelemetryStore;
        yield* store.record(successEvent);
        yield* store.record(errorEvent);
        return yield* store.recent({ limit: 10, interestingOnly: true });
      }).pipe(Effect.provide(TelemetryStoreMemory))
    );
    expect(out).toEqual([errorEvent]);
  });

  it("persists first-class error_tag and interesting columns", async () => {
    const clientLayer = SqliteClient.layer({ filename: ":memory:" });
    const layer = Layer.effect(TelemetryStore, make).pipe(
      Layer.provideMerge(clientLayer)
    );
    const rows = await Effect.runPromise(
      Effect.gen(function* () {
        const store = yield* TelemetryStore;
        const sql = yield* SqlClient.SqlClient;
        yield* store.record(errorEvent);
        return yield* sql<{
          error_tag: string;
          interesting: number;
        }>`SELECT error_tag, interesting FROM events WHERE id = ${errorEvent.id}`;
      }).pipe(Effect.provide(layer))
    );
    expect(rows[0]?.error_tag).toBe("BoomError");
    expect(rows[0]?.interesting).toBe(1);
  });
});
