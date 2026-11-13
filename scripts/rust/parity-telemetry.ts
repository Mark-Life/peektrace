/** Telemetry: `doctor` byte for byte over a seeded store, the opt-out rules,
 * and the shape of the events each CLI records, read back by the other
 * side's `doctor`. Fixtures only; every run gets its own `PEEKTRACE_DIR`. */
import { Database } from "bun:sqlite";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { join } from "node:path";
import {
  compareRun,
  fixtureEnv,
  fixturesDir,
  REPO,
  type Run,
  report,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { ORACLE_MODE, tsValueSync } from "./parity-oracle";
import { buildSeed, spawn } from "./parity-telemetry-seed";

const ROOT = join(WORK, "telemetry");
const SIDES = [
  ["ts", TS_CMD],
  ["rust", [rustBin]],
] as const;

/** Env with telemetry on: `fixtureEnv` without the opt-out. */
const onEnv = (home: string, extra: Record<string, string>) => {
  const { PEEKTRACE_NO_TELEMETRY: _off, ...env } = fixtureEnv(home, extra);
  return env as Record<string, string>;
};

const DOCTOR_CASES: readonly (readonly string[])[] = [
  ["doctor"],
  ["doctor", "--last", "2"],
  ["doctor", "--last", "0"],
  ["doctor", "--last", "-1"],
  ["doctor", "--last=3"],
  ["doctor", "--last", "1e1"],
  ["doctor", "--interesting-only"],
  ["doctor", "--interesting-only", "--last", "1"],
  ["doctor", "--out", "out.json"],
  ["doctor", "--out", "missing/x.json"],
  ["doctor", "--out", "adir"],
  ["doctor", "--out", ""],
  ["doctor", "--last", "abc"],
  ["doctor", "--last", "1.5"],
  ["--json", "doctor", "--last", "1"],
  ["--read-only", "doctor"],
];

const SIDE_FILES = /^telemetry\.db-(wal|shm)$/;
const GENERATED_AT = /"generatedAt": \d+/;
const EVENT_TS = /"ts": (\d+)/g;
const WROTE = /^Wrote (\d+) events/;

const sideFiles = (dir: string) =>
  readdirSync(dir)
    .filter((f) => !SIDE_FILES.test(f))
    .sort();

/** Bundles with the clock masked; with `now`, event times read as offsets
 * from it, so a recorded bundle matches one made later. */
const bundles = (dir: string, now?: number) =>
  sideFiles(dir)
    .filter((f) => f.endsWith(".json"))
    .map((f) => {
      const text = readFileSync(join(dir, f), "utf8").replace(
        GENERATED_AT,
        '"generatedAt": 0'
      );
      return [
        f,
        now === undefined
          ? text
          : text.replace(
              EVENT_TS,
              (_, ts) => `"ts": "now-${now - Number(ts)}"`
            ),
      ];
    });

/** The TS side's result: recorded and replayed under `key`. */
const tsSide = <T>(side: string, key: string, f: () => T) =>
  side === "ts" ? tsValueSync(key, f) : f();

/** A. `doctor` over copies of one seeded store. */
const runDoctor = (home: string) => {
  const seed = join(ROOT, "seed");
  const now = buildSeed(seed, home);
  const cases = [
    ...DOCTOR_CASES.map((args) => ({ args, fresh: false })),
    { args: ["doctor"], fresh: true },
  ];
  const doctorSide = (
    side: string,
    cmd: readonly string[],
    i: number,
    { args, fresh }: (typeof cases)[number]
  ) => {
    const dir = join(ROOT, "doctor", side, String(i));
    if (fresh) {
      mkdirSync(dir, { recursive: true });
    } else {
      cpSync(seed, dir, { recursive: true });
    }
    mkdirSync(join(dir, "adir"), { recursive: true });
    const r = spawn(
      [...cmd, ...args],
      fixtureEnv(home, { PEEKTRACE_DIR: dir }),
      dir
    );
    const norm = (s: string) => s.replaceAll(dir, "<dir>");
    return {
      run: { ...r, stdout: norm(r.stdout), stderr: norm(r.stderr) },
      files: sideFiles(dir).join(","),
      bundles: JSON.stringify(bundles(dir, now)),
    };
  };
  cases.forEach((c, i) => {
    const label = `telemetry doctor [${c.args.join(" ")}]${c.fresh ? " fresh" : ""}`;
    const [ts, rust] = SIDES.map(([side, cmd]) =>
      tsSide(side, label, () => doctorSide(side, cmd, i, c))
    ) as [ReturnType<typeof doctorSide>, ReturnType<typeof doctorSide>];
    compareRun(label, ts.run, rust.run);
    if (ts.files !== rust.files) {
      report(`${label}: files ts=${ts.files} rust=${rust.files}`);
    }
    if (ts.bundles !== rust.bundles) {
      report(`${label}: bundle differs`);
    }
  });
  return cases.length;
};

const OPT_OUT: readonly (readonly [
  Record<string, string>,
  readonly string[],
])[] = [
  [{}, ["sessions", "ls"]],
  [{}, ["--no-telemetry", "sessions", "ls"]],
  [{}, ["sessions", "ls", "--no-telemetry"]],
  [{}, ["--no-telemetry", "--telemetry", "sessions", "ls"]],
  [{}, ["--telemetry", "sessions", "ls"]],
  [{ PEEKTRACE_NO_TELEMETRY: "1" }, ["sessions", "ls"]],
  [{ PEEKTRACE_NO_TELEMETRY: "" }, ["sessions", "ls"]],
  [{}, ["statusline"]],
  [{}, ["--remote", "http://127.0.0.1:1", "statusline"]],
  [{}, ["--log-level", "info", "statusline"]],
];

const rowCount = (dir: string) => {
  const path = join(dir, "telemetry.db");
  if (!existsSync(path)) {
    return "no db";
  }
  const db = new Database(path, { readonly: true });
  const n = db.query("SELECT count(*) AS n FROM events").get() as { n: number };
  db.close();
  return `${n.n} rows`;
};

/** B. Which runs create the store and record. */
const runOptOut = (home: string, extra: Record<string, string>) => {
  OPT_OUT.forEach(([env, args], i) => {
    const key = `telemetry opt-out [${JSON.stringify(env)} ${args.join(" ")}]`;
    const [ts, rust] = SIDES.map(([side, cmd]) =>
      tsSide(side, key, () => {
        const dir = join(ROOT, "optout", side, String(i));
        spawn(
          [...cmd, ...args],
          { ...onEnv(home, { ...extra, PEEKTRACE_DIR: dir }), ...env },
          REPO,
          ""
        );
        return rowCount(dir);
      })
    );
    if (ts !== rust) {
      report(
        `telemetry opt-out [${JSON.stringify(env)} ${args.join(" ")}]: ts=${ts} rust=${rust}`
      );
    }
  });
  return OPT_OUT.length;
};

interface Stored {
  readonly attributes: Record<string, unknown>;
  readonly error?: Record<string, unknown>;
  readonly kind: string;
  readonly name: string;
  readonly outcome: string;
  readonly spans: readonly { readonly name: string }[];
}

const TRANSPORT = /^(RpcServer|RpcClient)\./;

/** What both sides must agree on, per event. */
const shapeOf = (data: string) => {
  const e = JSON.parse(data) as Stored;
  const attributes = Object.fromEntries(
    Object.entries(e.attributes).filter(
      ([k]) =>
        k !== "code.stacktrace" &&
        !(e.kind === "cli" && k === "sessions.unreadable")
    )
  );
  return JSON.stringify({
    keys: Object.keys(e),
    kind: e.kind,
    name: e.name,
    outcome: e.outcome,
    attributes,
    error:
      e.error === undefined
        ? undefined
        : { tag: "tag" in e.error, message: "message" in e.error },
    spans: [
      ...new Set(e.spans.map((s) => s.name).filter((n) => !TRANSPORT.test(n))),
    ].sort(),
  });
};

const events = (dir: string) => {
  const path = join(dir, "telemetry.db");
  if (!existsSync(path)) {
    return [];
  }
  const db = new Database(path, { readonly: true });
  const rows = db.query("SELECT data FROM events ORDER BY ts").all() as {
    data: string;
  }[];
  db.close();
  return rows.map((r) => r.data);
};

/** The schema text SQLite keeps, which both CLIs must write alike. */
const schema = (dir: string) => {
  const db = new Database(join(dir, "telemetry.db"), { readonly: true });
  const rows = db
    .query("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
    .all() as { sql: string }[];
  db.close();
  return rows.map((r) => r.sql).join("\n");
};

const mcpInput = `${[
  {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: { protocolVersion: "2025-06-18", capabilities: {} },
  },
  { jsonrpc: "2.0", method: "notifications/initialized" },
  {
    jsonrpc: "2.0",
    id: 2,
    method: "tools/call",
    params: { name: "sessions_list", arguments: {} },
  },
]
  .map((m) => JSON.stringify(m))
  .join("\n")}\n`;

/** C. Event shape per command, then each side's `doctor` over the other's store. */
const runShape = (home: string, extra: Record<string, string>) => {
  const listed = spawn(
    [rustBin, "--json", "sessions", "ls", "--limit", "1"],
    fixtureEnv(home, extra),
    REPO
  );
  const id =
    (JSON.parse(listed.stdout || "[]") as { id: string }[])[0]?.id ?? "missing";
  const cases: readonly (readonly [readonly string[], string?])[] = [
    [["sessions", "ls"]],
    [["--json", "sessions", "ls"]],
    [["sessions", "analyze", id]],
    [["sessions", "analyze", "nope"]],
    [["sessions", "ls", "--limit", "abc"]],
    [["memory", "ls"]],
    [["stats", "refresh"]],
    [["stats"]],
    [["--help"]],
    [[]],
    [["--version"]],
    [["sessions", "--help"]],
    [["mcp"], mcpInput],
  ];
  const dirs = {
    ts: join(ROOT, "shape", "ts"),
    rust: join(ROOT, "shape", "rust"),
  };
  for (const [args, stdin] of cases) {
    const label = `telemetry shape [${args.join(" ")}]`;
    const [ts, rust] = SIDES.map(([side, cmd]) =>
      tsSide(side, label, () => {
        const one = join(ROOT, "shape-one", side, args.join("_") || "bare");
        spawn(
          [...cmd, ...args],
          onEnv(home, { ...extra, PEEKTRACE_DIR: one }),
          REPO,
          stdin ?? ""
        );
        spawn(
          [...cmd, ...args],
          onEnv(home, { ...extra, PEEKTRACE_DIR: dirs[side] }),
          REPO,
          stdin ?? ""
        );
        return events(one).map(shapeOf);
      })
    ) as [string[], string[]];
    if (ts.length !== rust.length) {
      report(`${label}: events ts=${ts.length} rust=${rust.length}`);
      continue;
    }
    ts.forEach((t, i) => {
      if (t !== rust[i]) {
        report(`${label}: event ${i} ts=${t} rust=${rust[i]}`);
      }
    });
  }
  const tsSchema = tsValueSync("telemetry schema ts", () => schema(dirs.ts));
  if (tsSchema !== schema(dirs.rust)) {
    report(`telemetry schema: ts=${tsSchema} rust=${schema(dirs.rust)}`);
  }
  if (ORACLE_MODE === "replay") {
    console.log("telemetry cross-read: live only");
    return cases.length + 1;
  }
  const count = (r: Run) =>
    WROTE.exec(r.stdout)?.[1] ?? `exit ${r.code}: ${r.stdout}${r.stderr}`;
  const tsReads = spawn(
    [...TS_CMD, "doctor", "--last", "-1", "--out", join(ROOT, "ts-reads.json")],
    fixtureEnv(home, { PEEKTRACE_DIR: dirs.rust }),
    REPO
  );
  const rustReads = spawn(
    [rustBin, "doctor", "--last", "-1", "--out", join(ROOT, "rust-reads.json")],
    fixtureEnv(home, { PEEKTRACE_DIR: dirs.ts }),
    REPO
  );
  if (
    tsReads.code !== 0 ||
    rustReads.code !== 0 ||
    count(tsReads) !== count(rustReads)
  ) {
    report(
      `telemetry cross-read: ts doctor on rust db=${count(tsReads)} rust doctor on ts db=${count(rustReads)}`
    );
  }
  return cases.length + 1;
};

/** `doctor` over a copy of the caller's real store, so redaction meets
 * real command lines. The real files are only copied, never opened. */
const runRealDoctor = () => {
  const src = join(process.env.HOME ?? "", ".peektrace");
  if (!existsSync(join(src, "telemetry.db"))) {
    console.log("suite telemetry-real: skipped (no telemetry.db)");
    return;
  }
  const home = join(ROOT, "real-home");
  mkdirSync(home, { recursive: true });
  const snap = join(ROOT, "real-snap");
  mkdirSync(snap, { recursive: true });
  for (const f of ["telemetry.db", "telemetry.db-wal"]) {
    if (existsSync(join(src, f))) {
      cpSync(join(src, f), join(snap, f));
    }
  }
  const [ts, rust] = SIDES.map(([side, cmd]) => {
    const dir = join(ROOT, "real", side);
    cpSync(snap, dir, { recursive: true });
    const r = spawn(
      [...cmd, "doctor", "--last", "-1"],
      fixtureEnv(home, { PEEKTRACE_DIR: dir }),
      dir
    );
    const norm = (t: string) => t.replaceAll(dir, "<dir>");
    return {
      run: { ...r, stdout: norm(r.stdout), stderr: norm(r.stderr) },
      bundles: bundles(dir),
    };
  }) as [{ run: Run; bundles: string[][] }, { run: Run; bundles: string[][] }];
  compareRun("telemetry-real doctor", ts.run, rust.run);
  if (JSON.stringify(ts.bundles) !== JSON.stringify(rust.bundles)) {
    const [a, b] = [ts.bundles[0]?.[1] ?? "", rust.bundles[0]?.[1] ?? ""];
    const al = a.split("\n");
    const bl = b.split("\n");
    const at = al.findIndex((l, i) => l !== bl[i]);
    report(
      `telemetry-real doctor: bundle line ${at + 1}: ts=${JSON.stringify(al[at])} rust=${JSON.stringify(bl[at])}`
    );
  }
  console.log(
    `suite telemetry-real: ${WROTE.exec(ts.run.stdout)?.[1] ?? 0} events`
  );
};

export const runTelemetryStage = ({
  fixtures,
  real,
}: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  rmSync(ROOT, { recursive: true, force: true });
  const home = join(ROOT, "home");
  mkdirSync(home, { recursive: true });
  if (fixtures) {
    const corpus = fixturesDir();
    const extra = {
      PEEKTRACE_CLAUDE_PROJECTS: corpus,
      PEEKTRACE_CODEX_SESSIONS: corpus,
      PEEKTRACE_PI_SESSIONS: corpus,
      PEEKTRACE_NO_UPDATE_CHECK: "1",
    };
    const n = runDoctor(home) + runOptOut(home, extra) + runShape(home, extra);
    console.log(`suite telemetry: ${n} cases`);
  }
  if (real) {
    runRealDoctor();
  }
};
