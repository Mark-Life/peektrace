/** Parity stage for `sessions search`: the fixture suites, a synthetic
 * corpus (the `search.test.ts` one, under a fake HOME), generated edge lines,
 * and the real corpus (`--json` only).
 *
 * Normalisation: when TS reports `truncated`, its scan stopped early with up
 * to 8 loads still in flight, so `scannedSessions` and `unreadable` vary run
 * to run. Both are dropped from both sides then, with the stderr
 * `N sessions could not be read` line. Real runs also drop sessions written
 * to during the run. */
import { chmodSync, existsSync, mkdirSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  compareRun,
  firstLineDiff,
  fixtureEnv,
  fixturesDir,
  pool,
  type Run,
  report,
  run,
  runAsync,
  rustBin,
  type Suite,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { buildOpencodeData } from "./parity-opencode-fixtures";
import { storeStamp } from "./parity-opencode-stamp";
import { buildEdge, buildSynthetic, locked } from "./parity-search-fixtures";

const READ_WRITE = 0o644;
const SEARCH = ["--no-telemetry", "sessions", "search"];
const QUERIES = [
  "the",
  "THE CONFIG",
  "config loader",
  'a"b',
  "walrus",
  "quokka",
  "привет мир",
  "src/app.ts",
  "sk-ant-api03",
  "an old walrus",
  "k-sign",
  "s-long ß-sharp",
  "ΟΔΟΣ",
];
const UNREADABLE_LINE = /^\d+ sessions? could not be read\n/m;
const search = (...rest: string[]) => [...SEARCH, ...rest];

/** Args for one suite: every query in three renderings, then flags. */
const suiteArgs = (projects: readonly string[]) => [
  ...QUERIES.flatMap((q) => [
    search(q, "--json", "--limit", "0"),
    search(q, "--limit", "0"),
    ["--no-telemetry", "--pretty", "sessions", "search", q, "--limit", "0"],
  ]),
  search("--", "-v", "--json", "--limit", "0"),
  search("an", "old", "walrus", "--limit", "0"),
  search("the", "--limit", "1", "--json"),
  search("the", "--limit", "2"),
  search("walrus"),
  search("quokka", "--json"),
  ...["claude", "codex", "pi", "opencode", "all"].map((a) =>
    search("the", "--agent", a, "--json", "--limit", "0")
  ),
  ...projects.flatMap((p) => [
    search("the", `--project=${p}`, "--json", "--limit", "0"),
    search("walrus", `--project=${p}`, "--limit", "0"),
  ]),
  search("the", "--since", "2026-04-01", "--json", "--limit", "0"),
  search("the", "--since", "100000w", "--limit", "0"),
  search("sk-ant", "--no-redact", "--json", "--limit", "0"),
  search("walrus", "--no-redact", "--limit", "0"),
  search("zzz-no-such-text"),
  search("zzz-no-such-text", "--json"),
  search("walrus", "--agent", "bogus"),
  search("   "),
  search(""),
  search("walrus", "--limit", "-1"),
  search("walrus", "--limit", "1.5"),
  search("walrus", "--limit"),
  search("-rf"),
  search("--force", "--json"),
  search("git", "push", "--force", "--limit", "0"),
  search("push", "-rf", "--json", "--limit", "0"),
  search("--limit", "0", "--force", "--json"),
  search("-walrus", "--json"),
  search("walrus", "--since", "bogus"),
  search("walrus", "--since", "99999999999w"),
];

const truncated = (stdout: string) => {
  try {
    return (JSON.parse(stdout) as { truncated?: boolean }).truncated === true;
  } catch {
    return false;
  }
};

/** Drop what a truncated TS scan leaves to timing. */
const settle = (r: Run, json: boolean): Run => {
  const stderr = r.stderr.replace(UNREADABLE_LINE, "");
  if (!json) {
    return { ...r, stderr };
  }
  try {
    const v = JSON.parse(r.stdout) as Record<string, unknown>;
    v.scannedSessions = undefined;
    v.unreadable = undefined;
    return { ...r, stdout: `${JSON.stringify(v, null, 2)}\n`, stderr };
  } catch {
    return { ...r, stderr };
  }
};

const runCase = async (
  name: string,
  env: Record<string, string>,
  args: string[],
  serial = false
) => {
  const json = args.includes("--json");
  const earlyOf = (r: Run) =>
    json ? truncated(r.stdout) : r.stderr.includes("more sessions match");
  // Record-time canon: what a truncated scan leaves to timing is dropped.
  const canon = (r: Run) => (earlyOf(r) ? settle(r, json) : r);
  const tsRun = () =>
    runAsync([...TS_CMD, ...args], env, undefined, undefined, canon);
  const both = () => Promise.all([tsRun(), runAsync([rustBin, ...args], env)]);
  const [ts, rust] = serial
    ? [await tsRun(), await runAsync([rustBin, ...args], env)]
    : await both();
  const early = earlyOf(ts);
  const label = `${name} [${args.join(" ")}]`;
  if (early) {
    compareRun(label, settle(ts, json), settle(rust, json));
  } else {
    compareRun(label, ts, rust);
  }
};

/** Projects each suite filters on: raw slugs and their redacted forms. */
const projectsOf = (env: Record<string, string>) => {
  const out = run(
    [rustBin, ...SEARCH, "the", "--json", "--limit", "0", "--no-redact"],
    env
  );
  const raw =
    out.code === 0
      ? (JSON.parse(out.stdout) as { sessions: { project: string }[] })
      : { sessions: [] };
  const red = run([rustBin, ...SEARCH, "the", "--json", "--limit", "0"], env);
  const shown =
    red.code === 0
      ? (JSON.parse(red.stdout) as { sessions: { project: string }[] })
      : { sessions: [] };
  const picks = new Set(
    [...raw.sessions.slice(0, 2), ...shown.sessions.slice(0, 2)].map(
      (s) => s.project
    )
  );
  return [...picks, "-home-demo-work"];
};

const fixtureSuites = (suites: readonly Suite[]): Suite[] => {
  const emptyHome = join(WORK, "empty-home");
  mkdirSync(emptyHome, { recursive: true });
  const fixtures = fixturesDir();
  const reuse = suites.filter((s) =>
    ["agents-fixtures", "edge"].includes(s.name)
  );
  const ocMain = join(WORK, "oc", "main");
  if (!existsSync(ocMain)) {
    buildOpencodeData(
      join(WORK, "oc"),
      join(fixtures, "sessions/projects")
    ).close();
  }
  for (const dir of ["oc-home", "oc-empty"]) {
    mkdirSync(join(WORK, dir), { recursive: true });
  }
  const oc: Suite = {
    name: "search-oc-main",
    env: fixtureEnv(join(WORK, "oc-home"), {
      PEEKTRACE_CLAUDE_PROJECTS: join(WORK, "oc-empty"),
      PEEKTRACE_CODEX_SESSIONS: join(WORK, "oc-empty"),
      PEEKTRACE_PI_SESSIONS: join(WORK, "oc-empty"),
      PEEKTRACE_OPENCODE_DATA: ocMain,
    }),
    args: [],
  };
  return [
    {
      name: "search-ts-fixtures",
      env: fixtureEnv(emptyHome, {
        PEEKTRACE_CLAUDE_PROJECTS: fixtures,
        PEEKTRACE_CODEX_SESSIONS: fixtures,
        PEEKTRACE_PI_SESSIONS: fixtures,
        PEEKTRACE_OPENCODE_DATA: join(fixtures, "sessions-opencode"),
      }),
      args: [],
    },
    ...reuse.map((s) => ({ ...s, name: `search-${s.name}` })),
    oc,
    { name: "search-synthetic", env: fixtureEnv(buildSynthetic()), args: [] },
    { name: "search-lines", env: fixtureEnv(buildEdge()), args: [] },
  ];
};

const runFixtures = async (suites: readonly Suite[]) => {
  for (const suite of fixtureSuites(suites)) {
    const args = suiteArgs(projectsOf(suite.env));
    console.log(`suite ${suite.name}: ${args.length} cases`);
    // TS skips an OpenCode store another process holds locked, so runs
    // that share a store go one at a time.
    const shared = suite.env.PEEKTRACE_OPENCODE_DATA !== undefined;
    await pool(
      args,
      (a) => runCase(suite.name, suite.env, a, shared),
      shared ? 1 : undefined
    );
  }
};

// Real corpus

interface Listed {
  readonly agent: string;
  readonly id: string;
  readonly path: string;
}

interface Found {
  readonly agent: string;
  readonly id: string;
}

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

const stamps = (env: Record<string, string>) => {
  const out = run(
    [rustBin, "--no-telemetry", "--json", "sessions", "ls", "--limit", "0"],
    env
  );
  const rows = JSON.parse(out.stdout) as Listed[];
  return new Map(rows.map((r) => [`${r.agent}:${r.id}`, stampOf(r.path)]));
};

/** Each session's raw JSON text in 2-space `--json` search output. */
const chunks = (stdout: string) => {
  const out: string[] = [];
  let current: string[] | undefined;
  for (const text of stdout.split("\n")) {
    if (text === "    {") {
      current = [];
    } else if (current && (text === "    }" || text === "    },")) {
      out.push(current.join("\n"));
      current = undefined;
    } else {
      current?.push(text);
    }
  }
  return out;
};

const sessionsOf = (r: Run, live: ReadonlySet<string>) => {
  const v = JSON.parse(r.stdout) as { sessions: Found[] };
  const raw = chunks(r.stdout);
  return v.sessions
    .map((s, i) => ({ key: `${s.agent}:${s.id}`, raw: raw[i] ?? "" }))
    .filter((s) => !live.has(s.key));
};

const compareReal = (
  label: string,
  ts: Run,
  rust: Run,
  live: ReadonlySet<string>
) => {
  if (ts.code !== rust.code) {
    report(`${label}: exit ts=${ts.code} rust=${rust.code}`);
    return 0;
  }
  const a = sessionsOf(ts, live);
  const b = sessionsOf(rust, live);
  const n =
    live.size > 0 ? Math.min(a.length, b.length) : Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i]?.key !== b[i]?.key) {
      report(`${label}: session ${i} ts=${a[i]?.key} rust=${b[i]?.key}`);
      return 0;
    }
    if (a[i]?.raw !== b[i]?.raw) {
      report(
        `${label}: ${a[i]?.key} ${firstLineDiff(a[i]?.raw ?? "", b[i]?.raw ?? "")}`
      );
    }
  }
  if (live.size === 0) {
    const tail = (r: Run) => (truncated(r.stdout) ? settle(r, true) : r);
    const [t, r] = [tail(ts), tail(rust)];
    const meta = (s: string) => {
      const { sessions: _, ...rest } = JSON.parse(s) as Record<string, unknown>;
      return JSON.stringify(rest);
    };
    if (meta(t.stdout) !== meta(r.stdout)) {
      report(`${label}: ts=${meta(t.stdout)} rust=${meta(r.stdout)}`);
    }
    if (t.stderr !== r.stderr) {
      report(`${label}: stderr ${firstLineDiff(t.stderr, r.stderr)}`);
    }
  }
  return n;
};

const REAL_QUERIES = [
  "error",
  "parity harness",
  "TODO",
  "the",
  "ключ",
  '"',
  "sk-ant",
];
const REAL_AGENTS = ["all", "claude", "codex", "pi", "opencode"];

const runReal = async () => {
  const env = { ...process.env, PEEKTRACE_NO_TELEMETRY: "1" } as Record<
    string,
    string
  >;
  const cases = REAL_QUERIES.flatMap((q) =>
    REAL_AGENTS.flatMap((agent) =>
      ["0", "5"].map((limit) =>
        search(q, "--agent", agent, "--limit", limit, "--json")
      )
    )
  );
  console.log(`suite search-real: ${cases.length} cases`);
  // TS reads the OpenCode DB with busy timeout 0, so a lock held by another
  // case makes it skip the store. Those cases run alone, TS then Rust.
  const usesOpencode = (args: readonly string[]) => {
    const agent = args[args.indexOf("--agent") + 1];
    return agent === "all" || agent === "opencode";
  };
  const before = stamps(env);
  const pooled = await pool(
    cases.filter((args) => !usesOpencode(args)),
    async (args) => {
      const [ts, rust] = await Promise.all([
        runAsync([...TS_CMD, ...args], env),
        runAsync([rustBin, ...args], env),
      ]);
      return { args, ts, rust };
    },
    4
  );
  const alone: typeof pooled = [];
  for (const args of cases.filter(usesOpencode)) {
    const ts = await runAsync([...TS_CMD, ...args], env);
    const rust = await runAsync([rustBin, ...args], env);
    alone.push({ args, ts, rust });
  }
  const runs = [...pooled, ...alone];
  const after = stamps(env);
  const live = new Set(
    [...new Set([...before.keys(), ...after.keys()])].filter(
      (k) => before.get(k) !== after.get(k)
    )
  );
  let compared = 0;
  for (const { args, ts, rust } of runs) {
    compared += compareReal(`search-real [${args.join(" ")}]`, ts, rust, live);
  }
  console.log(
    `  compared ${compared} session results, ${live.size} live sessions skipped`
  );
};

export const runSearchStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
  readonly suites: readonly Suite[];
}) => {
  try {
    if (args.fixtures) {
      await runFixtures(args.suites);
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
