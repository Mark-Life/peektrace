/** Shared pieces of the parity harness: process runs, diff reporting and
 * fixture env. */
import { cpSync, existsSync, readdirSync, statSync, utimesSync } from "node:fs";
import { cpus, tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve } from "node:path";
import {
  digest,
  GOLDENS_ROOT,
  NO_GOLDEN,
  PINNED,
  setOracleRoots,
  tsValue,
  tsValueSync,
} from "./parity-oracle";

export const REPO = resolve(import.meta.dir, "../..");

const inside = (parent: string, child: string) => {
  const rel = relative(parent, child);
  return rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
};

/** The scratch dir `--work <dir>` names, or why it is refused. The dir is
 * wiped, so it must sit under `<repo>/.scratch`, the OS temp dir or the
 * goldens root and must not hold the repo. */
export const resolveWork = (
  argv: readonly string[],
  repo: string,
  tmp: string,
  goldens = GOLDENS_ROOT
): { readonly dir: string } | { readonly error: string } => {
  const at = argv.indexOf("--work");
  if (at < 0) {
    return { dir: join(repo, ".scratch", "parity") };
  }
  const value = argv[at + 1];
  if (value === undefined || value === "" || value.startsWith("--")) {
    return { error: "Missing value for --work." };
  }
  const dir = resolve(repo, value);
  const safe =
    (inside(join(repo, ".scratch"), dir) ||
      inside(tmp, dir) ||
      inside(goldens, dir)) &&
    dir !== repo &&
    !inside(dir, repo);
  return safe
    ? { dir }
    : {
        error: `Refusing --work ${dir}: it is wiped, so it must sit under ${join(repo, ".scratch")} or ${tmp}.`,
      };
};

const work = resolveWork(process.argv, REPO, tmpdir());
if ("error" in work) {
  console.error(work.error);
  process.exit(2);
}
/** Scratch dir, wiped at start; `--work <dir>` keeps two runs apart. */
export const WORK = work.dir;
export const TS_CMD = ["bun", join(REPO, "apps/cli/src/index.ts")];
const MAX_REPORTED = 20;
const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const REPO_FIXTURES = join(REPO, "packages/core/test/fixtures");

const pinTimes = (path: string) => {
  if (statSync(path).isDirectory()) {
    for (const name of readdirSync(path)) {
      pinTimes(join(path, name));
    }
  }
  utimesSync(path, FIXED_MTIME, FIXED_MTIME);
};

/** `packages/core/test/fixtures`; when goldens are recorded or replayed, a
 * copy in WORK with fixed mtimes (some rows print a file's mtime). */
export const fixturesDir = () => {
  if (!PINNED) {
    return REPO_FIXTURES;
  }
  const dir = join(WORK, "repo-fixtures");
  if (!existsSync(dir)) {
    cpSync(REPO_FIXTURES, dir, { recursive: true });
    pinTimes(dir);
  }
  return dir;
};

/** Set a generated fixture file's mtime to a fixed time when goldens are
 * recorded or replayed (rows with no timestamp print it). */
export const pinMtime = (path: string) => {
  if (PINNED) {
    utimesSync(path, FIXED_MTIME, FIXED_MTIME);
  }
};

/** Copy options that keep source mtimes when goldens are recorded or
 * replayed, so copies of pinned fixtures stay pinned. */
export const COPY = { recursive: true, preserveTimestamps: PINNED } as const;

export interface Run {
  readonly code: number;
  readonly stderr: string;
  readonly stdout: string;
}

export interface Suite {
  readonly args: readonly (readonly string[])[];
  readonly env: Record<string, string>;
  readonly name: string;
}

export const flag = (name: string) => process.argv.includes(name);
export const option = (name: string) => {
  const at = process.argv.indexOf(name);
  return at < 0 ? undefined : process.argv[at + 1];
};

export const rustBin = resolve(
  REPO,
  option("--bin") ?? "target/release/peektrace"
);

/** A TS failure: an Effect error log on stdout. */
export const DEFECT = /^\[\d\d:\d\d:\d\d\.\d{3}\] ERROR \(#/;
const LOG_TIME = /^\[\d\d:\d\d:\d\d\.\d{3}\]/;

const isTs = (cmd: readonly string[]) =>
  cmd[0] === TS_CMD[0] && cmd[1] === TS_CMD[1];

/** Env vars that do not change TS output and differ per machine. */
const MACHINE_ENV = new Set(["PATH", "BUN_RUNTIME_TRANSPILER_CACHE_PATH"]);

/** The oracle key of a TS run: its argv, plus a digest of env, cwd, stdin. */
export const runKey = (
  cmd: readonly string[],
  env: Record<string, string>,
  cwd: string,
  stdin?: string | Uint8Array
) => {
  const vars = Object.entries(env)
    .filter(([k]) => !MACHINE_ENV.has(k))
    .sort(([a], [b]) => (a < b ? -1 : 1));
  const input =
    stdin === undefined
      ? null
      : Bun.hash(typeof stdin === "string" ? stdin : stdin).toString(36);
  return `ts ${JSON.stringify(cmd.slice(2))} ${digest({ vars, cwd, input })}`;
};

/** Record-time canon of every TS run: a defect keeps only its first line,
 * with the clock zeroed (stages only match `DEFECT` on it). */
const canonRun = (r: Run): Run =>
  DEFECT.test(r.stdout)
    ? {
        ...r,
        stdout: `${(r.stdout.split("\n")[0] ?? "").replace(LOG_TIME, "[00:00:00.000]")}\n`,
      }
    : r;

export interface RunOpts {
  /** Record-time canon, after the defect one. */
  readonly canon?: (r: Run) => Run;
  readonly cwd?: string;
  readonly stdin?: string | Uint8Array;
}

const missingRun = (key: string): Run => ({
  code: NO_GOLDEN,
  stdout: "",
  stderr: `<no golden: ${key}>`,
});

const spawnSync = (
  cmd: readonly string[],
  env: Record<string, string>,
  o: RunOpts
): Run => {
  const out = Bun.spawnSync([...cmd], {
    env,
    cwd: o.cwd ?? REPO,
    stdin: o.stdin === undefined ? "ignore" : new Blob([o.stdin as BlobPart]),
  });
  return {
    code: out.exitCode ?? -1,
    stdout: out.stdout.toString(),
    stderr: out.stderr.toString(),
  };
};

const runOpts = (o: RunOpts, key: string) => ({
  canon: (r: Run) => (o.canon ? o.canon(canonRun(r)) : canonRun(r)),
  missing: missingRun(key),
});

/** One run. A TS run goes through the oracle: replay returns the golden. */
export const run = (
  cmd: readonly string[],
  env: Record<string, string>,
  o: RunOpts = {}
): Run => {
  if (!isTs(cmd)) {
    return spawnSync(cmd, env, o);
  }
  const key = runKey(cmd, env, o.cwd ?? REPO, o.stdin);
  return tsValueSync(key, () => spawnSync(cmd, env, o), runOpts(o, key));
};

/** One run without blocking, so many can go at once. */
export const runAsync = (
  cmd: readonly string[],
  env: Record<string, string>,
  stdin?: string,
  cwd = REPO,
  canon?: (r: Run) => Run
): Promise<Run> => {
  if (!isTs(cmd)) {
    return spawnAsync(cmd, env, stdin, cwd);
  }
  const key = runKey(cmd, env, cwd, stdin);
  return tsValue(
    key,
    () => spawnAsync(cmd, env, stdin, cwd),
    runOpts({ canon }, key)
  );
};

const spawnAsync = async (
  cmd: readonly string[],
  env: Record<string, string>,
  stdin: string | undefined,
  cwd: string
): Promise<Run> => {
  const proc = Bun.spawn([...cmd], {
    env,
    cwd,
    stdin: stdin === undefined ? "ignore" : new Blob([stdin]),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { code, stdout, stderr };
};

/** Map `items` through `f` with at most `limit` in flight, keeping order. */
export const pool = async <T, R>(
  items: readonly T[],
  f: (item: T) => Promise<R>,
  limit = Number(process.env.PARITY_JOBS) || Math.max(2, cpus().length)
): Promise<R[]> => {
  const out: R[] = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const at = next;
      next += 1;
      out[at] = await f(items[at] as T);
    }
  };
  await Promise.all(Array.from({ length: limit }, worker));
  return out;
};

export const diffs: string[] = [];
export const report = (line: string) => {
  diffs.push(line);
  if (diffs.length <= MAX_REPORTED) {
    console.log(`  DIFF ${line}`);
  }
};

setOracleRoots({ work: WORK, repo: REPO, report });

export const firstLineDiff = (a: string, b: string) => {
  const al = a.split("\n");
  const bl = b.split("\n");
  const at = al.findIndex((line, i) => line !== bl[i]);
  const i = at < 0 ? al.length : at;
  return `line ${i + 1}: ts=${JSON.stringify(al[i])} rust=${JSON.stringify(bl[i])}`;
};

/** Byte-exact comparison of one invocation. */
export const compareRun = (label: string, ts: Run, rust: Run) => {
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
export const fixtureEnv = (
  home: string,
  extra: Record<string, string> = {}
) => ({
  PATH: process.env.PATH ?? "",
  HOME: home,
  TZ: "UTC",
  PEEKTRACE_DIR: join(home, ".peektrace"),
  PEEKTRACE_NO_TELEMETRY: "1",
  BUN_RUNTIME_TRANSPILER_CACHE_PATH: "0",
  ...extra,
});

/** One stage's case counts and the known TS defects its fixtures must hit. */
export interface Tally {
  readonly known: ReadonlySet<string>;
  readonly seen: Set<string>;
  readonly stats: { cases: number; defects: number; live: number };
}

export const newTally = (known: readonly string[]): Tally => ({
  known: new Set(known),
  seen: new Set(),
  stats: { cases: 0, defects: 0, live: 0 },
});

/** A known defect TS no longer hits should be compared instead. */
export const reportUnseen = (t: Tally) => {
  for (const name of t.known) {
    if (!t.seen.has(name)) {
      report(`known TS defect ${name} no longer fails: compare it`);
    }
  }
};

export const tallyLine = (stage: string, { stats }: Tally) =>
  `${stage} stage: ${stats.cases} cases, ${stats.defects} skipped as TS defects, ${stats.live} skipped as live`;

/** The first `newest` rows as listed (newest first) plus the `largest`
 * biggest, one per path. */
export const newestAndLargest = <R extends { path: string; sizeBytes: number }>(
  rows: readonly R[],
  newest: number,
  largest: number,
  extra: readonly R[] = []
) => {
  const picked = [
    ...rows.slice(0, newest),
    ...[...rows].sort((a, b) => b.sizeBytes - a.sizeBytes).slice(0, largest),
    ...extra,
  ];
  return [...new Map(picked.map((r) => [r.path, r])).values()];
};
