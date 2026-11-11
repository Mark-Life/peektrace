/**
 * Parity harness: runs the TS CLI (`bun apps/cli/src/index.ts`) and the Rust
 * release binary with the same args and env, then diffs stdout, stderr and
 * exit codes. Exits 1 on any difference.
 *
 *   bun scripts/rust/parity.ts                 # fixtures + real ~/.claude
 *   bun scripts/rust/parity.ts --fixtures-only # CI
 *   bun scripts/rust/parity.ts --bin target/debug/peektrace
 *
 * Normalisation, kept to what the comparison needs:
 * - Fixture suites run with a clean env (`PATH`, a fake `HOME`, `TZ=UTC`).
 *   `TZ=UTC` is needed because JS reads an offset-less date-time as local
 *   time and Rust reads it as UTC; the edge fixture has such timestamps.
 * - Real corpus: sessions written to during the run (size or mtime changed
 *   between the first and last stat) are left out of the comparison and
 *   counted as "live". Only `--json` output is compared, row by row.
 */
import {
  chmodSync,
  existsSync,
  mkdirSync,
  rmSync,
  statSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { edgeFiles, edgeSettings } from "./parity-fixtures";

const REPO = resolve(import.meta.dir, "../..");
const WORK = join(REPO, ".scratch", "parity");
const TS_CMD = ["bun", join(REPO, "apps/cli/src/index.ts")];
const MAX_REPORTED = 20;
const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const NO_ACCESS = 0o000;
const READ_WRITE = 0o644;

interface Run {
  readonly code: number;
  readonly stderr: string;
  readonly stdout: string;
}

interface Suite {
  readonly args: readonly (readonly string[])[];
  readonly env: Record<string, string>;
  readonly name: string;
}

const flag = (name: string) => process.argv.includes(name);
const option = (name: string) => {
  const at = process.argv.indexOf(name);
  return at < 0 ? undefined : process.argv[at + 1];
};

const rustBin = resolve(REPO, option("--bin") ?? "target/release/peektrace");

const run = (cmd: readonly string[], env: Record<string, string>): Run => {
  const out = Bun.spawnSync([...cmd], { env, cwd: REPO });
  return {
    code: out.exitCode ?? -1,
    stdout: out.stdout.toString(),
    stderr: out.stderr.toString(),
  };
};

const diffs: string[] = [];
const report = (line: string) => {
  diffs.push(line);
  if (diffs.length <= MAX_REPORTED) {
    console.log(`  DIFF ${line}`);
  }
};

const firstLineDiff = (a: string, b: string) => {
  const al = a.split("\n");
  const bl = b.split("\n");
  const at = al.findIndex((line, i) => line !== bl[i]);
  const i = at < 0 ? al.length : at;
  return `line ${i + 1}: ts=${JSON.stringify(al[i])} rust=${JSON.stringify(bl[i])}`;
};

/** Byte-exact comparison of one invocation. */
const compareRun = (label: string, ts: Run, rust: Run) => {
  if (ts.code !== rust.code) {
    report(`${label}: exit ts=${ts.code} rust=${rust.code}`);
  }
  if (ts.stdout !== rust.stdout) {
    report(`${label}: stdout ${firstLineDiff(ts.stdout, rust.stdout)}`);
  }
  if (ts.stderr !== rust.stderr) {
    report(`${label}: stderr ${firstLineDiff(ts.stderr, rust.stderr)}`);
  }
};

/** A clean env for fixture runs: nothing from the caller leaks in. */
const fixtureEnv = (home: string, extra: Record<string, string> = {}) => ({
  PATH: process.env.PATH ?? "",
  HOME: home,
  TZ: "UTC",
  PEEKTRACE_DIR: join(home, ".peektrace"),
  PEEKTRACE_NO_TELEMETRY: "1",
  BUN_RUNTIME_TRANSPILER_CACHE_PATH: "0",
  ...extra,
});

const write = (path: string, body: string | Uint8Array) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
  utimesSync(path, FIXED_MTIME, FIXED_MTIME);
};

/** Build the generated edge-case tree; returns its HOME. */
const buildEdge = () => {
  const home = join(WORK, "edge-home");
  const projects = join(home, ".claude", "projects");
  for (const [rel, body] of Object.entries(edgeFiles)) {
    write(join(projects, rel), body);
  }
  const work = join(WORK, "work-home");
  write(
    join(work, "projects", "-work", "w1.jsonl"),
    '{"type":"user","timestamp":"2026-03-01T00:00:00.000Z"}\n'
  );
  mkdirSync(join(projects, "-edge-c", "dir.jsonl"), { recursive: true });
  mkdirSync(join(projects, "-edge-c", "nested", "deep"), { recursive: true });
  write(join(projects, "-edge-c", "nested", "deep", "x.jsonl"), "{}\n");
  symlinkSync(join(projects, "-edge-a"), join(projects, "-edge-link"));
  symlinkSync(
    join(projects, "-edge-a", "basic.jsonl"),
    join(projects, "-edge-c", "linked.jsonl")
  );
  symlinkSync(
    join(projects, "missing"),
    join(projects, "-edge-c", "dangling.jsonl")
  );
  const locked = join(projects, "-edge-c", "locked.jsonl");
  write(locked, '{"type":"user"}\n');
  chmodSync(locked, NO_ACCESS);
  write(
    join(home, ".peektrace", "settings.json"),
    edgeSettings(work, join(WORK, "missing-home"))
  );
  return home;
};

const LS = ["--no-telemetry", "sessions", "ls"];
const claudeArgs = (...rest: string[]) => [...LS, "--agent", "claude", ...rest];

const COMMON_ARGS: readonly (readonly string[])[] = [
  [
    "--no-telemetry",
    "--json",
    "sessions",
    "ls",
    "--agent",
    "claude",
    "--limit",
    "0",
  ],
  claudeArgs("--limit", "0"),
  [
    "--no-telemetry",
    "--pretty",
    "sessions",
    "ls",
    "--agent",
    "claude",
    "--limit",
    "0",
  ],
  [...LS, "--limit", "0"],
  claudeArgs(),
  claudeArgs("--json", "--limit", "2"),
  claudeArgs("--since", "2026-01-01", "--limit", "0"),
  claudeArgs("--since", "2026-06-01 10:00:00", "--limit", "0"),
  claudeArgs("--since", "2026-06-01T10:00:30+0000", "--limit", "0"),
  claudeArgs("--since", "100000w", "--limit", "0"),
  claudeArgs("--cwd", "/", "--limit", "0"),
  claudeArgs("--cwd", "/Users/demo/proj", "--limit", "0"),
  claudeArgs("--cwd", "/tmp/edge/", "--limit", "0"),
  claudeArgs("--project=-Users-demo-proj", "--limit", "0"),
  claudeArgs("--project", "sessions-claude", "--limit", "0"),
  claudeArgs("--project=-edge-a", "--json", "--limit", "0"),
  claudeArgs("--limit", "abc"),
  claudeArgs("--limit", "-1"),
  claudeArgs("--limit", "1e1"),
  [...LS, "--agent", "bogus"],
  claudeArgs("--since", "xx"),
  claudeArgs("--since", "2026-02-30"),
];

/** Run one fixture suite, every arg set, byte for byte. */
const runSuite = (suite: Suite) => {
  console.log(`suite ${suite.name}`);
  for (const args of suite.args) {
    const ts = run([...TS_CMD, ...args], suite.env);
    const rust = run([rustBin, ...args], suite.env);
    compareRun(`${suite.name} [${args.join(" ")}]`, ts, rust);
  }
};

const fixtureSuites = (): Suite[] => {
  const emptyHome = join(WORK, "empty-home");
  mkdirSync(emptyHome, { recursive: true });
  const fixtures = join(REPO, "packages/core/test/fixtures");
  return [
    {
      name: "ts-fixtures",
      env: fixtureEnv(emptyHome, { PEEKTRACE_CLAUDE_PROJECTS: fixtures }),
      args: COMMON_ARGS,
    },
    {
      name: "golden",
      env: fixtureEnv(emptyHome, {
        PEEKTRACE_CLAUDE_PROJECTS: join(fixtures, "sessions/projects"),
      }),
      args: COMMON_ARGS,
    },
    { name: "edge", env: fixtureEnv(buildEdge()), args: COMMON_ARGS },
  ];
};

interface Row {
  readonly path: string;
  readonly [key: string]: unknown;
}

/** The raw text of each top-level object in 2-space `--json` output. */
const rawChunks = (stdout: string) => {
  const chunks: string[] = [];
  let current: string[] | undefined;
  for (const text of stdout.split("\n")) {
    if (text === "  {") {
      current = [];
    } else if (text === "  }" || text === "  },") {
      chunks.push((current ?? []).join("\n"));
      current = undefined;
    } else {
      current?.push(text);
    }
  }
  return chunks;
};

/** Each row's raw `--json` text, keyed by path, so number formatting counts. */
const rawRows = (stdout: string) => {
  const rows = JSON.parse(stdout) as Row[];
  const chunks = rawChunks(stdout);
  return new Map(
    rows.map((r, i) => [r.path, { row: r, raw: chunks[i] ?? "" }])
  );
};

const stampOf = (path: string) => {
  try {
    const s = statSync(path);
    return `${s.size}:${s.mtimeMs}`;
  } catch {
    return "missing";
  }
};

const compareRows = (
  ts: Map<string, { row: Row; raw: string }>,
  rust: typeof ts,
  live: Set<string>
) => {
  let compared = 0;
  for (const [path, t] of ts) {
    const r = rust.get(path);
    if (live.has(path)) {
      continue;
    }
    if (!r) {
      report(`real: only in ts: ${path}`);
      continue;
    }
    compared += 1;
    const keys = (row: Row) => Object.keys(row).join(",");
    if (keys(t.row) !== keys(r.row)) {
      report(`real: ${path} keys ts=[${keys(t.row)}] rust=[${keys(r.row)}]`);
    }
    for (const key of new Set([...Object.keys(t.row), ...Object.keys(r.row)])) {
      const a = JSON.stringify(t.row[key]);
      const b = JSON.stringify(r.row[key]);
      if (a !== b) {
        report(`real: ${path} ${key} ts=${a} rust=${b}`);
      }
    }
    if (t.raw !== r.raw && keys(t.row) === keys(r.row)) {
      report(`real: ${path} raw text differs: ${firstLineDiff(t.raw, r.raw)}`);
    }
  }
  for (const path of rust.keys()) {
    if (!(ts.has(path) || live.has(path))) {
      report(`real: only in rust: ${path}`);
    }
  }
  return compared;
};

/** The real corpus under the caller's env, `--json` only. */
const runReal = () => {
  const env = { ...process.env, PEEKTRACE_NO_TELEMETRY: "1" } as Record<
    string,
    string
  >;
  const args = [
    "--no-telemetry",
    "--json",
    "sessions",
    "ls",
    "--agent",
    "claude",
    "--limit",
    "0",
  ];
  const pre = JSON.parse(run([rustBin, ...args], env).stdout) as Row[];
  const before = new Map(pre.map((r) => [r.path, stampOf(r.path)]));
  const ts = run([...TS_CMD, ...args], env);
  const rust = run([rustBin, ...args], env);
  const tsRows = rawRows(ts.stdout);
  const rustRows = rawRows(rust.stdout);
  const paths = new Set([
    ...before.keys(),
    ...tsRows.keys(),
    ...rustRows.keys(),
  ]);
  const live = new Set([...paths].filter((p) => before.get(p) !== stampOf(p)));
  console.log(
    `suite real: ts=${tsRows.size} rust=${rustRows.size} live=${live.size}`
  );
  if (ts.code !== rust.code) {
    report(`real: exit ts=${ts.code} rust=${rust.code}`);
  }
  if (ts.stderr !== rust.stderr && live.size === 0) {
    report(`real: stderr ${firstLineDiff(ts.stderr, rust.stderr)}`);
  }
  const order = (rows: typeof tsRows) =>
    [...rows.keys()].filter((p) => !live.has(p));
  const tsOrder = order(tsRows);
  const rustOrder = order(rustRows);
  const at = tsOrder.findIndex((p, i) => p !== rustOrder[i]);
  if (at >= 0) {
    report(
      `real: order differs at ${at}: ts=${tsOrder[at]} rust=${rustOrder[at]}`
    );
  }
  const compared = compareRows(tsRows, rustRows, live);
  console.log(`  compared ${compared} rows, skipped ${live.size} live`);
};

const main = () => {
  if (!existsSync(rustBin)) {
    console.error(`missing ${rustBin}: run cargo build --release`);
    process.exit(2);
  }
  rmSync(WORK, { recursive: true, force: true });
  mkdirSync(WORK, { recursive: true });
  if (!flag("--real-only")) {
    for (const suite of fixtureSuites()) {
      runSuite(suite);
    }
  }
  const realRoot = join(process.env.HOME ?? "", ".claude", "projects");
  if (!flag("--fixtures-only") && existsSync(realRoot)) {
    runReal();
  }
  const locked = join(WORK, "edge-home/.claude/projects/-edge-c/locked.jsonl");
  if (existsSync(locked)) {
    chmodSync(locked, READ_WRITE);
  }
  console.log(
    diffs.length === 0 ? "parity: OK" : `parity: ${diffs.length} difference(s)`
  );
  process.exit(diffs.length === 0 ? 0 : 1);
};

main();
