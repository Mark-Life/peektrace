/**
 * OpenCode stage of the parity harness: `sessions ls`, `analyze` and
 * `export` on OpenCode data dirs built by `parity-opencode-fixtures.ts`, and
 * on the real data dir when there is one.
 *
 * Normalisation:
 * - Codex and Pi roots point at an empty dir. So does the Claude root in
 *   every fixture suite but the mixed one.
 * - `ses_far` holds a time past what a JS `Date` holds: TS prints an Effect
 *   error, so Rust only has to succeed with parseable output.
 * - Real data: TS reads SQLite with no busy timeout, so a concurrent reader
 *   can make it miss a store; a TS "not found" is run again.
 * - Real data: newest and largest sessions only, unless `--real-all`.
 * - Real data: OpenCode may write while the stage runs. The `opencode*.db`
 *   and `-wal` files and the `storage/` tree are stamped around `ls`; if any
 *   changed, `ls` is compared row by row, leaving out sessions whose own
 *   stamp changed (see `parity-opencode-ls.ts`).
 *   Each analyze/export case is skipped as live when its own session's DB
 *   rows or tree files changed.
 */
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { runCase } from "./parity-analyze";
import { analyze, exportOf, targetCases } from "./parity-analyze-cases";
import {
  compareRun,
  DEFECT,
  fixtureEnv,
  fixturesDir,
  newestAndLargest,
  newTally,
  pool,
  report,
  reportUnseen,
  run,
  rustBin,
  type Tally,
  TS_CMD,
  tallyLine,
  WORK,
} from "./parity-lib";
import {
  buildOpencodeData,
  type OpencodeData,
} from "./parity-opencode-fixtures";
import { changedPaths, lsRowDiffs } from "./parity-opencode-ls";
import { openSessionStamper, storeStamp } from "./parity-opencode-stamp";

interface Row {
  readonly id: string;
  readonly path: string;
  readonly sizeBytes: number;
  readonly updatedAt: string;
}

type Env = Record<string, string>;
type Stamper = ReturnType<typeof openSessionStamper>;

const LS = ["--no-telemetry", "sessions", "ls"];
const oc = (...rest: string[]) => [...LS, "--agent", "opencode", ...rest];
const ALL = ["--limit", "0"];
/** Listed refs that no store loads: absent from `ls`, still resolvable. */
const UNLISTED = ["ses_badmsg", "ses_other_name"];
/** Fixture sessions the TS CLI fails on; see "Known differences" in the plan. */
const KNOWN_DEFECTS = ["ses_far"];
/** Real sessions compared without `--real-all`. */
const SAMPLE = { newest: 30, largest: 10 };

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
  const rust = run([rustBin, ...args], env);
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

/** Every mode for one session, plus its shortest unique prefix. */
const rowCases = (row: Row, allIds: readonly string[]) => {
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
};

const sessionCases = (rows: readonly Row[], allIds: readonly string[]) =>
  rows.flatMap((row) => rowCases(row, allIds));

/** Every id any agent lists in `env` (`ls` shows only loadable ones). */
const allIds = (env: Env, extra: readonly string[]) => {
  const out = run([rustBin, "--json", "sessions", "ls", ...ALL], env);
  const ids = (JSON.parse(out.stdout) as Row[]).map((r) => r.id);
  return [...new Set([...ids, ...extra])];
};

const runSuite = async (
  tally: Tally,
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
  await pool(cases, (c) => runCase(tally, name, env, c));
};

const fixtureStage = async (tally: Tally, data: OpencodeData) => {
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
    tally,
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
    tally,
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
  await runSuite(tally, "oc-xdg", xdgEnv, [oc("--json", ...ALL)], {});
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
  await runSuite(
    tally,
    "oc-roots",
    rootsEnv,
    [oc("--json", ...ALL), oc(...ALL)],
    {
      cases: [analyze("ses_ext", "--json"), analyze("ses_rich", "--json")],
    }
  );
  const farEnv = pin({ PEEKTRACE_OPENCODE_DATA: data.far });
  for (const args of [oc("--json", ...ALL), oc(...ALL)]) {
    lsCase("oc-far", farEnv, args);
  }
  for (const args of [analyze("ses_far", "--json"), analyze("ses_far")]) {
    await runCase(tally, "oc-far", farEnv, { args });
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

/** Real `ls --json` on both CLIs; row by row when OpenCode wrote meanwhile. */
const realLs = (dir: string, env: Env, stamper: Stamper) => {
  const args = ["--no-telemetry", "--json", ...oc(...ALL).slice(1)];
  const sessionsBefore = new Map(
    stamper.ids().map((id) => [id, stamper.stamp(id)])
  );
  const before = storeStamp(dir);
  const ts = run([...TS_CMD, ...args], env);
  const rust = run([rustBin, ...args], env);
  const live = storeStamp(dir) !== before;
  const tsRows = JSON.parse(ts.stdout) as Row[];
  const rustRows = JSON.parse(rust.stdout) as Row[];
  if (live) {
    const changed = changedPaths(
      [...tsRows, ...rustRows],
      sessionsBefore,
      stamper.stamp
    );
    console.log(
      `suite oc-real: ts=${tsRows.length} rust=${rustRows.length} live, skipped ${changed.size} changed`
    );
    if (ts.code !== rust.code) {
      report(`oc-real [ls]: exit ts=${ts.code} rust=${rust.code}`);
    }
    for (const line of lsRowDiffs(tsRows, rustRows, changed)) {
      report(`oc-real [ls]: ${line}`);
    }
  } else {
    console.log(`suite oc-real: ts=${tsRows.length} rust=${rustRows.length}`);
    compareRun("oc-real [ls]", ts, rust);
  }
  return rustRows;
};

const realCases = async (
  tally: Tally,
  env: Env,
  all: boolean,
  stamper: Stamper,
  rustRows: readonly Row[]
) => {
  const ids = allIds(env, []);
  const picked = all
    ? rustRows
    : newestAndLargest(rustRows, SAMPLE.newest, SAMPLE.largest);
  const owned = picked.map((row) => ({
    row,
    before: stamper.stamp(row.id),
    cases: rowCases(row, ids),
  }));
  const cases = owned.flatMap((o) => o.cases.map((c) => ({ ...o, c })));
  console.log(
    `suite oc-real analyze: ${picked.length} of ${rustRows.length} sessions, ${cases.length} cases`
  );
  // TS opens SQLite with no busy timeout, so a concurrent reader can make it
  // miss a store and answer "not found"; run it again then.
  const missed = (ts: { stderr: string }) => ts.stderr.includes(" not found ");
  await pool(cases, ({ row, before: was, c }) =>
    runCase(tally, "real", env, c, () => stamper.stamp(row.id) !== was, missed)
  );
};

const realStage = async (tally: Tally, all: boolean) => {
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
  const stamper = openSessionStamper(dir);
  try {
    await realCases(tally, env, all, stamper, realLs(dir, env, stamper));
  } finally {
    stamper.close();
  }
};

export const runOpencodeStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
  readonly realAll: boolean;
}) => {
  const tally = newTally(KNOWN_DEFECTS);
  if (args.fixtures) {
    const data = buildOpencodeData(
      join(WORK, "oc"),
      join(fixturesDir(), "sessions/projects")
    );
    try {
      await fixtureStage(tally, data);
    } finally {
      data.close();
    }
    reportUnseen(tally);
  }
  if (args.real) {
    await realStage(tally, args.realAll);
  }
  console.log(tallyLine("opencode", tally));
};
