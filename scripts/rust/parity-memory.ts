/** Parity stage for `memory ls|show|rm`: the generated memory HOME, `rm` on
 * twin copies with a tree diff after, and the real `~/.claude` (read-only
 * commands, plus `rm` on a copy of one project's memory dir).
 *
 * Kept out of the fixtures: a frontmatter `type` named `__proto__` or after
 * an `Object.prototype` member (TS drops or stringifies it in `typeCounts`),
 * and an IO failure in `rm` (TS prints an Effect error dump). */
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  readlinkSync,
} from "node:fs";
import { join } from "node:path";
import {
  compareRun,
  fixtureEnv,
  pool,
  type Run,
  report,
  run,
  runAsync,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";
import {
  buildMemHome,
  EXTRA_NAMES,
  EXTRA_SLUGS,
} from "./parity-memory-fixtures";

const READ_WRITE = 0o644;
const BASE = ["--no-telemetry"];
const locked: string[] = [];

interface Vault {
  readonly entries: readonly { slug: string }[];
  readonly graph: {
    readonly edges: readonly { kind: string; resolvedTo?: string }[];
  };
}

const views = (cmd: readonly string[], rest: readonly string[]) => [
  [...BASE, "memory", ...cmd, "--", ...rest],
  [...BASE, "memory", ...cmd, "--json", "--", ...rest],
  [...BASE, "--pretty", "memory", ...cmd, "--", ...rest],
];

/** Every `ls` and `show` case for the projects TS lists, plus extras. */
const readCases = (env: Record<string, string>) => {
  const listed = JSON.parse(
    run([...TS_CMD, ...BASE, "memory", "ls", "--json"], env).stdout
  ) as { slug: string }[];
  const slugs = [...listed.map((p) => p.slug), ...EXTRA_SLUGS];
  const shows = listed.flatMap((p) => {
    const vault = JSON.parse(
      run([rustBin, ...BASE, "memory", "ls", "--json", "--", p.slug], env)
        .stdout
    ) as Vault;
    return [...vault.entries.map((e) => e.slug), ...EXTRA_NAMES].flatMap((n) =>
      views(["show"], [p.slug, n])
    );
  });
  return [
    [...BASE, "memory", "ls"],
    [...BASE, "memory", "ls", "--json"],
    [...BASE, "--pretty", "memory", "ls"],
    ...slugs.flatMap((s) => views(["ls"], [s])),
    ...shows,
  ];
};

/** Arg sets where only the exit code is compared: clap and `@effect/cli`
 * word usage errors differently. */
const USAGE = [
  [...BASE, "memory", "show", "--", "-Users-demo-proj-a"],
  [...BASE, "memory", "ls", "a", "b"],
  [...BASE, "memory"],
];

const runFixtures = async () => {
  const home = join(WORK, "mem-home");
  locked.push(...buildMemHome(home));
  const env = fixtureEnv(home);
  const cases = readCases(env);
  console.log(`suite memory-fixtures: ${cases.length + USAGE.length} cases`);
  await pool(cases, async (args) => {
    const [ts, rust] = await Promise.all([
      runAsync([...TS_CMD, ...args], env),
      runAsync([rustBin, ...args], env),
    ]);
    compareRun(`memory-fixtures [${args.join(" ")}]`, ts, rust);
  });
  for (const args of USAGE) {
    const ts = run([...TS_CMD, ...args], env);
    const rust = run([rustBin, ...args], env);
    if (ts.code !== rust.code) {
      report(
        `memory-usage [${args.join(" ")}]: exit ts=${ts.code} rust=${rust.code}`
      );
    }
  }
};

/** Path, kind, link target and bytes of every entry under `root`, except
 * the settings dir (it names the root). */
const tree = (root: string, dir = ""): string[] =>
  readdirSync(join(root, dir))
    .filter((name) => name !== ".peektrace")
    .sort()
    .flatMap((name) => {
      const rel = join(dir, name);
      const full = join(root, rel);
      const st = lstatSync(full);
      if (st.isSymbolicLink()) {
        return [`${rel} -> ${readlinkSync(full).replace(root, "<HOME>")}`];
      }
      if (st.isDirectory()) {
        return [`${rel}/`, ...tree(root, rel)];
      }
      return [`${rel} ${readFileSync(full).toString("base64")}`];
    });

const diffTrees = (label: string, a: string, b: string) => {
  const ta = tree(a);
  const tb = tree(b);
  const at = ta.findIndex((line, i) => line !== tb[i]);
  if (at >= 0 || ta.length !== tb.length) {
    const i = at < 0 ? Math.min(ta.length, tb.length) : at;
    report(
      `${label}: tree ts=${ta[i]?.slice(0, 120)} rust=${tb[i]?.slice(0, 120)}`
    );
  }
};

const homed = (r: Run, home: string): Run => ({
  code: r.code,
  stdout: r.stdout.replaceAll(home, "<HOME>"),
  stderr: r.stderr.replaceAll(home, "<HOME>"),
});

/** Run `ops` in order, TS on one copy and Rust on the other. */
const runTwins = (
  name: string,
  homes: { ts: string; rs: string },
  ops: readonly (readonly string[])[]
) => {
  for (const args of ops) {
    const ts = run([...TS_CMD, ...args], fixtureEnv(homes.ts));
    const rust = run([rustBin, ...args], fixtureEnv(homes.rs));
    compareRun(
      `${name} [${args.join(" ")}]`,
      homed(ts, homes.ts),
      homed(rust, homes.rs)
    );
  }
};

const rm = (project: string, name: string, ...flags: string[]) => [
  ...BASE,
  "memory",
  "rm",
  ...flags,
  "--",
  project,
  name,
];

const RM_OPS = [
  rm("-Users-demo-proj-a", "nested"),
  rm("-Users-demo-proj-a", "self", "--json"),
  rm("-work-proj", "w"),
  rm("-Users-demo-proj-a", "ghost"),
  rm("-Users-demo-proj-a", "ghost", "--json"),
  rm("-Users-demo-proj-a", "upper"),
  rm("-Users-demo-proj-a", "Upper"),
  rm("-Users-demo-proj-a", "link"),
  rm("-Users-demo-proj-a", "../../x"),
  rm("../../outside", "x"),
  rm("-Users-demo-proj-a", "flat", "--read-only"),
  [...BASE, "--read-only", "memory", "rm", "--", "-Users-demo-proj-a", "flat"],
  rm("C#proj", "hash"),
  rm("-Users-demo-seed", "b"),
  rm("-Users-demo-seed", "fx-nested", "--json"),
];

const runRm = () => {
  const homes = { ts: join(WORK, "mem-rm-ts"), rs: join(WORK, "mem-rm-rs") };
  locked.push(...buildMemHome(homes.ts), ...buildMemHome(homes.rs));
  console.log(`suite memory-rm: ${RM_OPS.length} cases`);
  runTwins("memory-rm", homes, RM_OPS);
  for (const path of locked.filter((p) => existsSync(p))) {
    chmodSync(path, READ_WRITE);
  }
  diffTrees("memory-rm", homes.ts, homes.rs);
};

/** Compare once more on a mismatch: Claude may write memory mid-run. */
const compareSettled = async (
  label: string,
  env: Record<string, string>,
  args: string[]
) => {
  const once = () =>
    Promise.all([
      runAsync([...TS_CMD, ...args], env),
      runAsync([rustBin, ...args], env),
    ]);
  let [ts, rust] = await once();
  if (
    ts.stdout !== rust.stdout ||
    ts.stderr !== rust.stderr ||
    ts.code !== rust.code
  ) {
    [ts, rust] = await once();
  }
  compareRun(label, ts, rust);
};

const inbound = (v: Vault) => {
  const counts = new Map<string, number>();
  for (const e of v.graph.edges) {
    if (e.kind !== "index" && e.resolvedTo !== undefined) {
      counts.set(e.resolvedTo, (counts.get(e.resolvedTo) ?? 0) + 1);
    }
  }
  return [...counts].sort((a, b) => b[1] - a[1])[0];
};

/** Copy one project's memory dir (an APFS clone on macOS). */
const copyMemory = (from: string, home: string, slug: string) => {
  const to = join(home, ".claude", "projects", slug);
  mkdirSync(to, { recursive: true });
  const flags = process.platform === "darwin" ? ["-c", "-R"] : ["-R"];
  execFileSync("cp", [...flags, from, join(to, "memory")]);
};

const runReal = async () => {
  const env = { ...process.env, PEEKTRACE_NO_TELEMETRY: "1" } as Record<
    string,
    string
  >;
  const listed = JSON.parse(
    run([rustBin, ...BASE, "memory", "ls", "--json"], env).stdout
  ) as { slug: string; memoryDir: string }[];
  const vaults = listed.map((p) => ({
    ...p,
    vault: JSON.parse(
      run([rustBin, ...BASE, "memory", "ls", "--json", "--", p.slug], env)
        .stdout
    ) as Vault,
  }));
  const cases = [
    [...BASE, "memory", "ls", "--json"],
    [...BASE, "memory", "ls"],
    ...vaults.flatMap((p) => [
      [...BASE, "memory", "ls", "--json", "--", p.slug],
      [...BASE, "memory", "ls", "--", p.slug],
      ...p.vault.entries.map((e) => [
        ...BASE,
        "memory",
        "show",
        "--json",
        "--",
        p.slug,
        e.slug,
      ]),
    ]),
  ];
  console.log(`suite memory-real: ${cases.length} cases`);
  await pool(
    cases,
    (args) => compareSettled(`memory-real [${args.join(" ")}]`, env, args),
    4
  );
  const pick = vaults
    .map((p) => ({ ...p, top: inbound(p.vault) }))
    .filter((p) => p.top !== undefined)
    .sort((a, b) => (b.top?.[1] ?? 0) - (a.top?.[1] ?? 0))[0];
  if (pick?.top === undefined) {
    console.log("suite memory-real-rm: skipped (no memory with inbound links)");
    return;
  }
  const homes = {
    ts: join(WORK, "real-mem-ts"),
    rs: join(WORK, "real-mem-rs"),
  };
  copyMemory(pick.memoryDir, homes.ts, pick.slug);
  copyMemory(pick.memoryDir, homes.rs, pick.slug);
  console.log(
    `suite memory-real-rm: ${pick.top[0]} (${pick.top[1]} inbound links)`
  );
  runTwins("memory-real-rm", homes, [
    rm(pick.slug, pick.top[0]),
    rm(pick.slug, pick.top[0], "--json"),
  ]);
  diffTrees("memory-real-rm", homes.ts, homes.rs);
};

export const runMemoryStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  try {
    if (args.fixtures) {
      await runFixtures();
      runRm();
    }
    if (args.real) {
      await runReal();
    }
  } finally {
    for (const path of locked.filter((p) => existsSync(p))) {
      chmodSync(path, READ_WRITE);
    }
  }
};
