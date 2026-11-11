/**
 * OpenCode stage of the parity harness: `sessions ls`, `analyze` and
 * `export` on OpenCode data dirs built by `parity-opencode-fixtures.ts`, and
 * on the real data dir when there is one.
 *
 * Normalisation:
 * - Codex and Pi roots point at an empty dir. Without `--agent` Rust prints
 *   SKIPPED_NOTE for them; that line is dropped.
 * - `ses_far` holds a time past what a JS `Date` holds: TS prints an Effect
 *   error, so Rust only has to succeed with parseable output.
 * - Real data: TS reads SQLite with no busy timeout, so a concurrent reader
 *   can make it miss a store; a TS "not found" is run again.
 * - Real data: OpenCode may write while the stage runs. The `opencode*.db`
 *   and `-wal` files and the `storage/` tree are stamped before and after; if
 *   any changed, `ls` compares only rows with the same `updatedAt` in both
 *   runs, and analyze/export cases are skipped as live.
 */
import {
  existsSync,
  mkdirSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { runCase } from "./parity-analyze";
import { targetCases } from "./parity-analyze-cases";
import {
  compareRun,
  DEFECT,
  firstLineDiff,
  fixtureEnv,
  pool,
  REPO,
  report,
  run,
  rustBin,
  SKIPPED_NOTE,
  TS_CMD,
  WORK,
} from "./parity-lib";
import {
  buildOpencodeData,
  type OpencodeData,
} from "./parity-opencode-fixtures";

interface Row {
  readonly id: string;
  readonly path: string;
  readonly sizeBytes: number;
  readonly updatedAt: string;
}

type Env = Record<string, string>;

const LS = ["--no-telemetry", "sessions", "ls"];
const oc = (...rest: string[]) => [...LS, "--agent", "opencode", ...rest];
const ALL = ["--limit", "0"];
/** Listed refs that no store loads: absent from `ls`, still resolvable. */
const UNLISTED = ["ses_badmsg", "ses_other_name"];

const LS_ARGS: readonly (readonly string[])[] = [
  ["--no-telemetry", "--json", "sessions", "ls", "--agent", "opencode", ...ALL],
  oc(...ALL),
  [
    "--no-telemetry",
    "--pretty",
    "sessions",
    "ls",
    "--agent",
    "opencode",
    ...ALL,
  ],
  oc("--limit", "2"),
  oc("--json", "--limit", "2"),
  oc("--project", "proj", ...ALL),
  oc("--project=/", ...ALL),
  oc("--project", "42", "--json", ...ALL),
  oc("--since", "1970-01-01T00:00:05Z", ...ALL),
  oc("--cwd", "/w", ...ALL),
  oc("--cwd", "/", "--json", ...ALL),
  [...LS, ...ALL],
  [...LS, "--json", ...ALL],
];

/** One `ls` run on both CLIs, byte for byte. */
const lsCase = (suite: string, env: Env, args: readonly string[]) => {
  const ts = run([...TS_CMD, ...args], env);
  const raw = run([rustBin, ...args], env);
  const rust = args.includes("--agent")
    ? raw
    : { ...raw, stderr: raw.stderr.replace(SKIPPED_NOTE, "") };
  const label = `${suite} [${args.join(" ")}]`;
  if (DEFECT.test(ts.stdout)) {
    if (rust.code !== 0 || rust.stdout === "" || DEFECT.test(rust.stdout)) {
      report(`${label}: on TS defect rust exit=${rust.code}`);
    }
    return;
  }
  compareRun(label, ts, rust);
};

const listRows = (env: Env) =>
  JSON.parse(
    run([rustBin, "--json", ...oc(...ALL).slice(1)], env).stdout
  ) as Row[];

/** Shortest prefix of `id` that no other id starts with. */
const uniquePrefix = (id: string, ids: readonly string[]) => {
  for (let n = 1; n < id.length; n++) {
    const p = id.slice(0, n);
    if (ids.filter((x) => x.startsWith(p)).length === 1) {
      return p;
    }
  }
  return;
};

const analyze = (...rest: string[]) => ["sessions", "analyze", ...rest];
const exportOf = (...rest: string[]) => ["sessions", "export", ...rest];

/** Every mode per session, plus its shortest unique prefix. */
const sessionCases = (rows: readonly Row[], allIds: readonly string[]) =>
  rows.flatMap((row) => {
    const prefix = uniquePrefix(row.id, allIds);
    return [
      ...targetCases(row, allIds),
      ...(prefix
        ? [
            { args: analyze(prefix, "--json") },
            { args: exportOf(prefix, "--format", "md") },
          ]
        : []),
    ];
  });

/** Every id any agent lists in `env` (`ls` shows only loadable ones). */
const allIds = (env: Env, extra: readonly string[]) => {
  const out = run([rustBin, "--json", "sessions", "ls", ...ALL], env);
  const ids = (JSON.parse(out.stdout) as Row[]).map((r) => r.id);
  return [...new Set([...ids, ...extra])];
};

const runSuite = async (
  name: string,
  env: Env,
  lsArgs: readonly (readonly string[])[],
  extra: {
    readonly dataDir?: string;
    readonly unlisted?: readonly string[];
    readonly cases?: readonly string[][];
  }
) => {
  for (const args of lsArgs) {
    lsCase(name, env, args);
  }
  const rows = listRows(env);
  const ids = allIds(env, extra.unlisted ?? []);
  const unlisted = (extra.unlisted ?? []).map((id) => ({
    id,
    path: `${extra.dataDir ?? ""}#${id}`,
    sizeBytes: 0,
    updatedAt: "",
  }));
  const cases = [
    ...sessionCases([...rows, ...unlisted], ids),
    ...(extra.cases ?? []).map((args) => ({ args })),
  ];
  console.log(`suite ${name}: ${rows.length} sessions, ${cases.length} cases`);
  await pool(cases, (c) => runCase(name, env, c));
};

const fixtureStage = async (data: OpencodeData) => {
  const empty = join(WORK, "oc-empty");
  const home = join(WORK, "oc-home");
  mkdirSync(empty, { recursive: true });
  mkdirSync(home, { recursive: true });
  const pin = (extra: Env) =>
    fixtureEnv(home, {
      PEEKTRACE_CLAUDE_PROJECTS: empty,
      PEEKTRACE_CODEX_SESSIONS: empty,
      PEEKTRACE_PI_SESSIONS: empty,
      ...extra,
    });
  writeFileSync(join(data.main, "ses_rich.jsonl"), "not read\n");
  await runSuite(
    "oc-main",
    pin({ PEEKTRACE_OPENCODE_DATA: data.main }),
    LS_ARGS,
    {
      dataDir: data.main,
      unlisted: UNLISTED,
      cases: [
        analyze(join(data.main, "ses_rich.jsonl"), "--json"),
        analyze("x", "--parent", "ses_rich"),
        analyze("ses_t"),
        analyze("--latest"),
        analyze("--latest", "--json"),
        analyze("--cwd", "/w"),
        exportOf("--cwd", "/w/b", "--format", "json"),
      ],
    }
  );
  await runSuite(
    "oc-mixed",
    pin({
      PEEKTRACE_CLAUDE_PROJECTS: data.mixedClaude,
      PEEKTRACE_OPENCODE_DATA: data.mixed,
    }),
    [
      [...LS, "--json", ...ALL],
      [...LS, ...ALL],
    ],
    {
      cases: [
        analyze("ses_amb"),
        analyze("ses_amb1", "--json"),
        analyze("--latest", "--json"),
        analyze("--cwd", "/w/amb"),
        analyze("zzz"),
      ],
    }
  );
  const xdgEnv = pin({ XDG_DATA_HOME: data.xdg });
  await runSuite("oc-xdg", xdgEnv, [oc("--json", ...ALL)], {});
  const rootsHome = join(WORK, "oc-roots-home");
  mkdirSync(join(rootsHome, ".peektrace"), { recursive: true });
  writeFileSync(
    join(rootsHome, ".peektrace", "settings.json"),
    JSON.stringify({
      roots: { opencode: [{ path: data.main }, { path: data.extra }] },
    })
  );
  const rootsEnv = {
    ...pin({ PEEKTRACE_OPENCODE_DATA: data.main }),
    HOME: rootsHome,
    PEEKTRACE_DIR: join(rootsHome, ".peektrace"),
  };
  await runSuite("oc-roots", rootsEnv, [oc("--json", ...ALL), oc(...ALL)], {
    cases: [analyze("ses_ext", "--json"), analyze("ses_rich", "--json")],
  });
  const farEnv = pin({ PEEKTRACE_OPENCODE_DATA: data.far });
  for (const args of [oc("--json", ...ALL), oc(...ALL)]) {
    lsCase("oc-far", farEnv, args);
  }
  for (const args of [analyze("ses_far", "--json"), analyze("ses_far")]) {
    await runCase("oc-far", farEnv, { args });
  }
};

/** The real data dir as the TS CLI resolves it. */
const realDataDir = () => {
  const env = process.env;
  if (env.PEEKTRACE_OPENCODE_DATA) {
    return env.PEEKTRACE_OPENCODE_DATA;
  }
  return env.XDG_DATA_HOME
    ? join(env.XDG_DATA_HOME, "opencode")
    : join(env.HOME ?? "", ".local", "share", "opencode");
};

/** Size and mtime of the stores and the tree. `-shm` changes on every read. */
const stampOf = (dir: string) => {
  const parts: string[] = [];
  const walk = (p: string) => {
    try {
      const s = statSync(p);
      if (s.isDirectory()) {
        for (const name of readdirSync(p).sort()) {
          walk(join(p, name));
        }
      } else {
        parts.push(`${p}:${s.size}:${s.mtimeMs}`);
      }
    } catch {
      parts.push(`${p}:missing`);
    }
  };
  for (const name of readdirSync(dir).sort()) {
    if (name.startsWith("opencode") && !name.endsWith("-shm")) {
      walk(join(dir, name));
    }
  }
  walk(join(dir, "storage"));
  return parts.join("|");
};

const realStage = async () => {
  const dir = realDataDir();
  if (!existsSync(dir)) {
    return;
  }
  const empty = join(WORK, "oc-empty");
  mkdirSync(empty, { recursive: true });
  const env = {
    ...process.env,
    PEEKTRACE_CODEX_SESSIONS: empty,
    PEEKTRACE_PI_SESSIONS: empty,
    PEEKTRACE_NO_TELEMETRY: "1",
  } as Env;
  const args = ["--no-telemetry", "--json", ...oc(...ALL).slice(1)];
  const before = stampOf(dir);
  const ts = run([...TS_CMD, ...args], env);
  const rust = run([rustBin, ...args], env);
  const live = stampOf(dir) !== before;
  const tsRows = JSON.parse(ts.stdout) as Row[];
  const rustRows = JSON.parse(rust.stdout) as Row[];
  console.log(
    `suite oc-real: ts=${tsRows.length} rust=${rustRows.length} live=${live}`
  );
  if (live) {
    const byPath = new Map(rustRows.map((r) => [r.path, r]));
    for (const t of tsRows) {
      const r = byPath.get(t.path);
      if (
        r &&
        r.updatedAt === t.updatedAt &&
        JSON.stringify(r) !== JSON.stringify(t)
      ) {
        report(
          `oc-real: ${t.path} ${firstLineDiff(JSON.stringify(t), JSON.stringify(r))}`
        );
      }
    }
  } else {
    compareRun("oc-real [ls]", ts, rust);
  }
  const ids = allIds(env, []);
  const cases = sessionCases(rustRows, ids);
  console.log(
    `suite oc-real analyze: ${rustRows.length} sessions, ${cases.length} cases`
  );
  // TS opens SQLite with no busy timeout, so a concurrent reader can make it
  // miss a store and answer "not found"; run it again then.
  const missed = (ts: { stderr: string }) => ts.stderr.includes(" not found ");
  await pool(cases, (c) =>
    runCase("real", env, c, () => stampOf(dir) !== before, missed)
  );
};

export const runOpencodeStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  if (args.fixtures) {
    const data = buildOpencodeData(
      join(WORK, "oc"),
      join(REPO, "packages/core/test/fixtures/sessions/projects")
    );
    try {
      await fixtureStage(data);
    } finally {
      data.close();
    }
  }
  if (args.real) {
    await realStage();
  }
};
