/** Shared pieces of the parity harness: process runs, diff reporting and
 * fixture env. */
import { cpus, tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve } from "node:path";

export const REPO = resolve(import.meta.dir, "../..");

const inside = (parent: string, child: string) => {
  const rel = relative(parent, child);
  return rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
};

/** The scratch dir `--work <dir>` names, or why it is refused. The dir is
 * wiped, so it must sit under `<repo>/.scratch` or the OS temp dir and must
 * not hold the repo. */
export const resolveWork = (
  argv: readonly string[],
  repo: string,
  tmp: string
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
    (inside(join(repo, ".scratch"), dir) || inside(tmp, dir)) &&
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

export const run = (
  cmd: readonly string[],
  env: Record<string, string>
): Run => {
  const out = Bun.spawnSync([...cmd], { env, cwd: REPO });
  return {
    code: out.exitCode ?? -1,
    stdout: out.stdout.toString(),
    stderr: out.stderr.toString(),
  };
};

/** One run without blocking, so many can go at once. */
export const runAsync = async (
  cmd: readonly string[],
  env: Record<string, string>
): Promise<Run> => {
  const proc = Bun.spawn([...cmd], {
    env,
    cwd: REPO,
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
  limit = Math.max(2, cpus().length)
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

/** A TS failure: an Effect error log on stdout. */
export const DEFECT = /^\[\d\d:\d\d:\d\d\.\d{3}\] ERROR \(#/;

export const diffs: string[] = [];
export const report = (line: string) => {
  diffs.push(line);
  if (diffs.length <= MAX_REPORTED) {
    console.log(`  DIFF ${line}`);
  }
};

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
