/**
 * Parity harness: runs the TS CLI (`bun apps/cli/src/index.ts`) and the Rust
 * release binary with the same args and env, then diffs stdout, stderr and
 * exit codes. Exits 1 on any difference.
 *
 *   bun scripts/rust/parity.ts                 # fixtures + real ~/.claude
 *   bun scripts/rust/parity.ts --fixtures-only # CI
 *   bun scripts/rust/parity.ts --bin target/debug/peektrace
 *   bun scripts/rust/parity.ts --real-all      # every real session in the
 *                                              # analyze/export stages
 *
 * Normalisation, kept to what the comparison needs:
 * - Fixture suites run with a clean env (`PATH`, a fake `HOME`, `TZ=UTC`).
 *   The edge suite runs again under `TZ=Asia/Tokyo`, because both CLIs read
 *   an offset-less date-time as local time and the edge fixture has some.
 * - OpenCode suites run after the `ls` suites; see `parity-opencode.ts`.
 * - Real corpus: sessions written to during the run (size or mtime changed
 *   between the first and last stat) are left out of the comparison and
 *   counted as "live". An OpenCode row is live when its whole store changed.
 *   Only `--json` output is compared, row by row.
 * - `sessions analyze` and `sessions export` run after the `ls` suites; see
 *   `parity-analyze.ts` for their cases and normalisation.
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
import { dirname, join } from "node:path";
import {
  agentsSettings,
  buildAgentsEdge,
  buildPiRoot,
} from "./parity-agents-fixtures";
import { runAnalyzeStage } from "./parity-analyze";
import { edgeFiles, edgeSettings } from "./parity-fixtures";
import {
  compareRun,
  diffs,
  firstLineDiff,
  fixtureEnv,
  flag,
  REPO,
  report,
  run,
  rustBin,
  type Suite,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { runOpencodeStage } from "./parity-opencode";
import { storeStamp } from "./parity-opencode-stamp";

const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const NO_ACCESS = 0o000;
const READ_WRITE = 0o644;
const AGENTS = ["claude", "codex", "pi"] as const;
/** Files the edge trees make unreadable; restored at the end. */
const locked: string[] = [];

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
  const claudeLocked = join(projects, "-edge-c", "locked.jsonl");
  write(claudeLocked, '{"type":"user"}\n');
  chmodSync(claudeLocked, NO_ACCESS);
  const extra = join(WORK, "extra-roots");
  locked.push(claudeLocked, ...buildAgentsEdge(home, extra));
  const missing = join(WORK, "missing-home");
  write(
    join(home, ".peektrace", "settings.json"),
    edgeSettings(work, missing, agentsSettings(extra, missing))
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
  ...["codex", "pi"].flatMap((agent) => {
    const args = (...rest: string[]) => [...LS, "--agent", agent, ...rest];
    return [
      args("--json", "--limit", "0"),
      args("--limit", "0"),
      ["--no-telemetry", "--pretty", ...args("--limit", "0").slice(1)],
      args("--since", "2026-01-01", "--limit", "0"),
      args("--cwd", "/", "--limit", "0"),
      args("--cwd", "/tmp/demo", "--json", "--limit", "0"),
    ];
  }),
  [...LS, "--json", "--limit", "0"],
  [...LS, "--project", "demo", "--json", "--limit", "0"],
  [...LS, "--project=--tmp-pi--", "--limit", "0"],
  [...LS, "--project=-Users-demo-proj", "--limit", "0"],
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
  const edgeHome = buildEdge();
  return [
    {
      name: "ts-fixtures",
      env: fixtureEnv(emptyHome, {
        PEEKTRACE_CLAUDE_PROJECTS: fixtures,
        PEEKTRACE_CODEX_SESSIONS: fixtures,
        PEEKTRACE_PI_SESSIONS: fixtures,
      }),
      args: COMMON_ARGS,
    },
    {
      name: "agents-fixtures",
      env: fixtureEnv(emptyHome, {
        PEEKTRACE_CLAUDE_PROJECTS: join(fixtures, "sessions/projects"),
        PEEKTRACE_CODEX_SESSIONS: join(fixtures, "sessions-codex"),
        PEEKTRACE_PI_SESSIONS: buildPiRoot(fixtures, join(WORK, "pi-root")),
      }),
      args: COMMON_ARGS,
    },
    {
      name: "golden",
      env: fixtureEnv(emptyHome, {
        PEEKTRACE_CLAUDE_PROJECTS: join(fixtures, "sessions/projects"),
      }),
      args: COMMON_ARGS,
    },
    { name: "edge", env: fixtureEnv(edgeHome), args: COMMON_ARGS },
    {
      name: "edge-tokyo",
      env: fixtureEnv(edgeHome, { TZ: "Asia/Tokyo" }),
      args: COMMON_ARGS,
    },
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

/** An OpenCode row's path, `<data dir>#<session id>`; other agents' rows are
 * `.jsonl` files. */
const OPENCODE_ROW = /#[^/\\]+(?<!\.jsonl)$/;

const stampOf = (path: string) => {
  const at = path.search(OPENCODE_ROW);
  if (at >= 0) {
    return storeStamp(path.slice(0, at));
  }
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

/** The real corpus under the caller's env, `--json` only: each agent, then
 * all of them. */
const runReal = () => {
  const env = { ...process.env, PEEKTRACE_NO_TELEMETRY: "1" } as Record<
    string,
    string
  >;
  for (const agent of [...AGENTS, undefined]) {
    runRealOnce(env, agent);
  }
};

const runRealOnce = (env: Record<string, string>, agent?: string) => {
  const name = `real ${agent ?? "all"}`;
  const args = [
    "--no-telemetry",
    "--json",
    "sessions",
    "ls",
    ...(agent ? ["--agent", agent] : []),
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
    `suite ${name}: ts=${tsRows.size} rust=${rustRows.size} live=${live.size}`
  );
  if (ts.code !== rust.code) {
    report(`${name}: exit ts=${ts.code} rust=${rust.code}`);
  }
  if (ts.stderr !== rust.stderr && live.size === 0) {
    report(`${name}: stderr ${firstLineDiff(ts.stderr, rust.stderr)}`);
  }
  const order = (rows: typeof tsRows) =>
    [...rows.keys()].filter((p) => !live.has(p));
  const tsOrder = order(tsRows);
  const rustOrder = order(rustRows);
  const at = tsOrder.findIndex((p, i) => p !== rustOrder[i]);
  if (at >= 0) {
    report(
      `${name}: order differs at ${at}: ts=${tsOrder[at]} rust=${rustOrder[at]}`
    );
  }
  const compared = compareRows(tsRows, rustRows, live);
  console.log(`  compared ${compared} rows, skipped ${live.size} live`);
};

const main = async () => {
  if (!existsSync(rustBin)) {
    console.error(`missing ${rustBin}: run cargo build --release`);
    process.exit(2);
  }
  rmSync(WORK, { recursive: true, force: true });
  mkdirSync(WORK, { recursive: true });
  const suites = fixtureSuites();
  if (!flag("--real-only")) {
    for (const suite of suites) {
      runSuite(suite);
    }
  }
  const realRoot = join(process.env.HOME ?? "", ".claude", "projects");
  const real = !flag("--fixtures-only") && existsSync(realRoot);
  if (real) {
    runReal();
  }
  await runOpencodeStage({
    fixtures: !flag("--real-only"),
    real: !flag("--fixtures-only"),
    realAll: flag("--real-all"),
  });
  await runAnalyzeStage({
    fixtures: flag("--real-only") ? [] : suites,
    real,
    realAll: flag("--real-all"),
  });
  for (const path of locked.filter((p) => existsSync(p))) {
    chmodSync(path, READ_WRITE);
  }
  console.log(
    diffs.length === 0 ? "parity: OK" : `parity: ${diffs.length} difference(s)`
  );
  process.exit(diffs.length === 0 ? 0 : 1);
};

await main();
