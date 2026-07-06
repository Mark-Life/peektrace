import { mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { SqlClient } from "@effect/sql";
import { SqliteClient } from "@effect/sql-sqlite-bun";
import { Context, Effect, Layer, Schema } from "effect";
import { isInteresting } from "./interesting";
import { WideEvent } from "./schema";

export interface TelemetryStore {
  readonly recent: (o: {
    limit: number;
    interestingOnly: boolean;
  }) => Effect.Effect<readonly WideEvent[]>;
  readonly record: (e: WideEvent) => Effect.Effect<void>;
}
export const TelemetryStore = Context.GenericTag<TelemetryStore>(
  "peephole/TelemetryStore"
);

const RETENTION_DAYS = 30;
const MAX_ROWS = 50_000;
const MS_PER_DAY = 86_400_000;
const decodeEvent = Schema.decodeUnknown(Schema.parseJson(WideEvent));

export const make = Effect.gen(function* () {
  const sql = yield* SqlClient.SqlClient;
  yield* sql`CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL,
    outcome TEXT NOT NULL, error_tag TEXT, duration_ms INTEGER NOT NULL,
    interesting INTEGER NOT NULL, data TEXT NOT NULL)`;
  yield* sql`CREATE INDEX IF NOT EXISTS idx_events_ts          ON events(ts)`;
  yield* sql`CREATE INDEX IF NOT EXISTS idx_events_interesting ON events(interesting, ts)`;
  yield* sql`CREATE INDEX IF NOT EXISTS idx_events_outcome     ON events(outcome, ts)`;

  const cutoff = Date.now() - RETENTION_DAYS * MS_PER_DAY;
  yield* sql`DELETE FROM events WHERE ts < ${cutoff}`;
  yield* sql`DELETE FROM events WHERE id NOT IN (SELECT id FROM events ORDER BY ts DESC LIMIT ${MAX_ROWS})`;

  const store: TelemetryStore = {
    record: (e) =>
      sql`INSERT OR REPLACE INTO events ${sql.insert({
        id: e.id,
        ts: e.ts,
        kind: e.kind,
        name: e.name,
        outcome: e.outcome,
        error_tag: e.outcome === "error" ? e.error.tag : null,
        duration_ms: e.durationMs,
        interesting: isInteresting(e) ? 1 : 0,
        data: JSON.stringify(e),
      })}`.pipe(Effect.ignore),
    recent: ({ limit, interestingOnly }) =>
      sql<{ data: string }>`
        SELECT data FROM events
        WHERE ${interestingOnly ? sql`interesting = 1` : sql`1 = 1`}
        ORDER BY ts DESC LIMIT ${limit}`.pipe(
        Effect.flatMap((rows) =>
          Effect.forEach(rows, (r) => decodeEvent(r.data))
        ),
        Effect.orDie
      ),
  };
  return store;
});

const dbDir = process.env.PEEPHOLE_DIR ?? join(homedir(), ".peephole");
const ClientLive = Layer.unwrapEffect(
  Effect.sync(() => {
    mkdirSync(dbDir, { recursive: true });
    return SqliteClient.layer({
      filename: join(dbDir, "telemetry.db"),
      create: true,
    });
  })
);
export const TelemetryStoreLive = Layer.effect(TelemetryStore, make).pipe(
  Layer.provide(ClientLive)
);
export const TelemetryStoreMemory = Layer.effect(TelemetryStore, make).pipe(
  Layer.provide(SqliteClient.layer({ filename: ":memory:" }))
);
