/** The seeded telemetry store `parity-telemetry.ts` runs `doctor` over:
 * events in the TS schema covering each redaction rule, every outcome,
 * odd numbers and keys, and one row past the retention window. */
import { Database } from "bun:sqlite";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { FAKE_SECRETS } from "./parity-fake-secrets";
import { fixtureEnv, REPO, type Run, report, TS_CMD } from "./parity-lib";
import { ORACLE_MODE, tsValueSync } from "./parity-oracle";

export const spawn = (
  cmd: readonly string[],
  env: Record<string, string>,
  cwd: string,
  stdin?: string
): Run => {
  const out = Bun.spawnSync([...cmd], {
    env,
    cwd,
    stdin: stdin === undefined ? "ignore" : new Blob([stdin]),
  });
  return {
    code: out.exitCode ?? -1,
    stdout: out.stdout.toString(),
    stderr: out.stderr.toString(),
  };
};

const DAY_MS = 86_400_000;
const SLOW_MS = 2000;

interface Seed {
  readonly data: string;
  readonly durationMs: number;
  readonly errorTag: string | null;
  readonly id: string;
  readonly kind: string;
  readonly name: string;
  readonly outcome: string;
  readonly ts: number;
}

const secret = FAKE_SECRETS.anthropicTelemetry;
const bare = "Zx9Qp2Lm7Rt4Vb8Nc3Kd6Wf1";

/** Events in the TS schema, newest last; `extra` adds raw JSON text. */
const seedEvents = (now: number): Seed[] => {
  const at = (i: number) => now - 100_000 + i * 1000;
  const base = (i: number, over: Record<string, unknown>) => ({
    id: `00000000-0000-4000-8000-${String(i).padStart(12, "0")}`,
    traceId: "0000000000000001",
    ts: at(i),
    kind: "cli",
    name: "cli",
    appVersion: "0.0.0-dev",
    platform: "darwin; bun/1.3.14; node/v24.3.0",
    durationMs: 12.5,
    attributes: { argv: "sessions ls" },
    spans: [],
    outcome: "success",
    ...over,
  });
  const events: Record<string, unknown>[] = [
    base(1, {
      spans: [
        {
          name: "AgentRegistry.listSessionFiles",
          durationMs: 1.25,
          attributes: { agent: "claude" },
        },
        {
          name: "Sessions.list",
          durationMs: 3,
          attributes: { malformedLines: 0 },
        },
      ],
    }),
    base(2, {
      attributes: { argv: "sessions analyze nope", "code.stacktrace": "at x" },
      outcome: "error",
      error: {
        tag: "SessionNotFoundError",
        message: '{ "id": "nope", "searchedRoot": "all agents" }',
        fields: { id: "nope", searchedRoot: "all agents" },
      },
    }),
    base(3, {
      attributes: { argv: "serve" },
      outcome: "defect",
      error: { message: "All fibers interrupted without errors." },
    }),
    base(4, { durationMs: 2500 }),
    base(5, {
      kind: "rpc",
      name: "rpc.sessions.listing",
      attributes: { "sessions.unreadable": 5 },
    }),
    base(6, {
      kind: "rpc",
      name: "rpc.sessions.listing",
      attributes: { "sessions.unreadable": 2 },
    }),
    base(7, { attributes: { argv: "sessions ls", token: secret } }),
    base(8, {
      attributes: {
        argv: `sessions analyze /Users/someone/proj2/a1b2c3d4e5f6g7h8i9j0k1l2.jsonl ${FAKE_SECRETS.githubTelemetry} ${bare}`,
      },
    }),
    base(9, {
      attributes: {
        authorization: bare,
        apiKeys: [[bare, "short"], { inner: bare, password: [bare] }],
        message: bare,
      },
      spans: [
        {
          name: "Fs.readText",
          durationMs: 0.5,
          attributes: { path: `/home/u/${bare}`, credentials: bare },
        },
      ],
    }),
    base(10, {
      attributes: { argv: "memory ls", note: "ключ — 日本語 😀  " },
    }),
  ];
  const numbers = `{"id":"num","traceId":"t","ts":${at(11)},"kind":"cli","name":"cli","appVersion":"v","platform":"p","durationMs":0.1,"attributes":{"a":0.1,"b":1e21,"c":-0,"d":9007199254740993,"e":1e-7},"spans":[],"outcome":"success"}`;
  const odd = `{"zz":1,"outcome":"error","traceId":"t","id":"odd","spans":[{"x":1,"attributes":{},"durationMs":2,"name":"c"}],"ts":${at(12)},"kind":"rpc","name":"n","appVersion":"v","platform":"p","durationMs":1,"attributes":{"b":1,"a":2},"error":{"tag":"T","message":"m","extra":true}}`;
  const stray = `{"id":"stray","traceId":"t","ts":${at(13)},"kind":"cli","name":"cli","appVersion":"v","platform":"p","durationMs":1,"attributes":{},"spans":[],"outcome":"success","error":{"message":"m"}}`;
  const old = `{"id":"old","traceId":"t","ts":${now - 31 * DAY_MS},"kind":"cli","name":"cli","appVersion":"v","platform":"p","durationMs":1,"attributes":{},"spans":[],"outcome":"success"}`;
  const row = (data: string): Seed => {
    const e = JSON.parse(data) as {
      id: string;
      ts: number;
      kind: string;
      name: string;
      outcome: string;
      durationMs: number;
      error?: { tag?: string };
    };
    return {
      data,
      durationMs: e.durationMs,
      errorTag: e.outcome === "error" ? (e.error?.tag ?? null) : null,
      id: e.id,
      kind: e.kind,
      name: e.name,
      outcome: e.outcome,
      ts: e.ts,
    };
  };
  return [
    ...events.map((e) => row(JSON.stringify(e))),
    row(numbers),
    row(odd),
    row(stray),
    row(old),
  ];
};

const STORE_FILES = ["telemetry.db", "telemetry.db-wal"];

/** The store files TS made, base64; replay writes them back. */
const storeOf = (dir: string) =>
  Object.fromEntries(
    STORE_FILES.filter((f) => existsSync(join(dir, f))).map((f) => [
      f,
      readFileSync(join(dir, f)).toString("base64"),
    ])
  );

/** A store made by TS, then filled with the seed rows; returns the clock
 * the rows are relative to. */
export const buildSeed = (dir: string, home: string) => {
  mkdirSync(dir, { recursive: true });
  const { made, files } = tsValueSync("telemetry seed store", () => ({
    made: spawn(
      [...TS_CMD, "doctor", "--out", join(dir, "made.json")],
      fixtureEnv(home, { PEEKTRACE_DIR: dir }),
      REPO
    ),
    files: storeOf(dir),
  }));
  if (ORACLE_MODE === "replay") {
    for (const [f, b64] of Object.entries(files)) {
      writeFileSync(join(dir, f), Buffer.from(b64, "base64"));
    }
  }
  if (made.code !== 0) {
    report(`telemetry seed: TS doctor failed: ${made.stdout}${made.stderr}`);
  }
  const db = new Database(join(dir, "telemetry.db"));
  const insert = db.prepare(
    "INSERT OR REPLACE INTO events (id,ts,kind,name,outcome,error_tag,duration_ms,interesting,data) VALUES (?,?,?,?,?,?,?,?,?)"
  );
  const now = Date.now();
  for (const s of seedEvents(now)) {
    const interesting = s.outcome !== "success" || s.durationMs > SLOW_MS;
    insert.run(
      s.id,
      s.ts,
      s.kind,
      s.name,
      s.outcome,
      s.errorTag,
      s.durationMs,
      interesting ? 1 : 0,
      s.data
    );
  }
  db.close();
  rmSync(join(dir, "made.json"), { force: true });
  return now;
};
