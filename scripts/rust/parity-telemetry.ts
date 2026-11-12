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
  REPO,
  type Run,
  report,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";
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
const WROTE = /^Wrote (\d+) events/;

const sideFiles = (dir: string) =>
  readdirSync(dir)
    .filter((f) => !SIDE_FILES.test(f))
    .sort();

const bundles = (dir: string) =>
  sideFiles(dir)
    .filter((f) => f.endsWith(".json"))
    .map((f) => [
      f,
      readFileSync(join(dir, f), "utf8").replace(
        GENERATED_AT,
        '"generatedAt": 0'
      ),
    ]);

/** A. `doctor` over copies of one seeded store. */
const runDoctor = (home: string) => {
  const seed = join(ROOT, "seed");
  buildSeed(seed, home);
  const cases = [
    ...DOCTOR_CASES.map((args) => ({ args, fresh: false })),
    { args: ["doctor"], fresh: true },
  ];
  cases.forEach(({ args, fresh }, i) => {
    const label = `telemetry doctor [${args.join(" ")}]${fresh ? " fresh" : ""}`;
    const runs = SIDES.map(([side, cmd]) => {
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
        bundles: JSON.stringify(bundles(dir)),
      };
    });
    const [ts, rust] = runs as [(typeof runs)[0], (typeof runs)[0]];
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
    const [ts, rust] = SIDES.map(([side, cmd]) => {
      const dir = join(ROOT, "optout", side, String(i));
      spawn(
        [...cmd, ...args],
        { ...onEnv(home, { ...extra, PEEKTRACE_DIR: dir }), ...env },
        REPO,
        ""
      );
      return rowCount(dir);
    });
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
    const [ts, rust] = SIDES.map(([side, cmd]) => {
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
    }) as [string[], string[]];
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

export const runTelemetryStage = () => {
  rmSync(ROOT, { recursive: true, force: true });
  const home = join(ROOT, "home");
  mkdirSync(home, { recursive: true });
  const fixtures = join(REPO, "packages/core/test/fixtures");
  const extra = {
    PEEKTRACE_CLAUDE_PROJECTS: fixtures,
    PEEKTRACE_CODEX_SESSIONS: fixtures,
    PEEKTRACE_PI_SESSIONS: fixtures,
    PEEKTRACE_NO_UPDATE_CHECK: "1",
  };
  const n = runDoctor(home) + runOptOut(home, extra) + runShape(home, extra);
  console.log(`suite telemetry: ${n} cases`);
};
