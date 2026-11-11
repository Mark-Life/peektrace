/**
 * Analyze/export stage of the parity harness. Every listed Claude session
 * runs through `sessions analyze` and `sessions export` in every output mode,
 * addressed by full id, short id and path, plus its subagents via `--parent`
 * and a fixed set of target and flag errors.
 *
 * Normalisation:
 * - Codex, Pi and OpenCode roots point at an empty dir, so TS prefix lookups
 *   and `--latest` see Claude sessions only, as the Rust CLI does.
 * - `exportedAt` (JSON), the `Exported:` line (md) and `<dt>Exported</dt>`
 *   (html) are the only time-based values; their stamp is masked.
 * - A TS defect (an Effect stack on stdout, e.g. a `null` row) skips the case
 *   and is counted.
 * - Real corpus: a sample by default (newest, largest, with subagents, with
 *   compaction); `--real-all` runs every session. Cases whose transcript or
 *   subagent tree changed during the run are skipped as live.
 */
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { buildAnalyzeEdge, LOCKED } from "./parity-analyze-fixtures";
import {
  compareRun,
  firstLineDiff,
  fixtureEnv,
  pool,
  type Run,
  report,
  runAsync,
  rustBin,
  type Suite,
  TS_CMD,
  WORK,
} from "./parity-lib";

const READ_WRITE = 0o644;
const DEFECT = /^\[\d\d:\d\d:\d\d\.\d{3}\] ERROR \(#/;
const JSONL = /\.jsonl$/;
const STAMP = /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/;
const SUBAGENTS_PER_SESSION = 3;
const SAMPLE = { newest: 30, largest: 10, subagents: 20, compaction: 10 };

interface Case {
  readonly args: readonly string[];
  /** File the command writes, compared too. */
  readonly out?: string;
}

interface Row {
  readonly id: string;
  readonly path: string;
  readonly sizeBytes: number;
}

const stats = { cases: 0, defects: 0, live: 0 };

/** Mask the export stamp on the lines that carry it. */
const mask = (text: string) =>
  text
    .split("\n")
    .map((line) =>
      line.includes('"exportedAt": "') ||
      line.startsWith("- Exported: ") ||
      line.startsWith("<dl>")
        ? line.replace(new RegExp(STAMP, "g"), "<stamp>")
        : line
    )
    .join("\n");

const masked = (r: Run): Run => ({ ...r, stdout: mask(r.stdout) });

/** The first JSON path where two documents differ, for a readable report. */
const jsonPath = (a: unknown, b: unknown, path = "$"): string | undefined => {
  if (typeof a !== typeof b || Array.isArray(a) !== Array.isArray(b)) {
    return path;
  }
  if (a === null || b === null || typeof a !== "object") {
    return Object.is(a, b) ? undefined : path;
  }
  const ka = Object.keys(a as object);
  const kb = Object.keys(b as object);
  if (ka.join(",") !== kb.join(",")) {
    return `${path} keys`;
  }
  for (const k of ka) {
    const sub = jsonPath(
      (a as Record<string, unknown>)[k],
      (b as Record<string, unknown>)[k],
      `${path}.${k}`
    );
    if (sub) {
      return sub;
    }
  }
  return;
};

const jsonHint = (ts: string, rust: string) => {
  try {
    return ` at ${jsonPath(JSON.parse(ts), JSON.parse(rust)) ?? "$ (text only)"}`;
  } catch {
    return "";
  }
};

const readOut = (path: string) => {
  try {
    return mask(readFileSync(path, "utf8"));
  } catch {
    return "<missing>";
  }
};

/** Run one case on both CLIs and compare; `live` skips a changed session. */
const runCase = async (
  suite: string,
  env: Record<string, string>,
  c: Case,
  live?: () => boolean
) => {
  const label = `${suite} [${c.args.join(" ")}]`;
  const tsArgs = [...TS_CMD, "--no-telemetry", ...c.args];
  const rustArgs = [rustBin, "--no-telemetry", ...c.args];
  let ts: Run;
  let rust: Run;
  let files = ["", ""];
  if (c.out) {
    // One path for both, so the `Wrote <path>` line matches.
    ts = await runAsync(tsArgs, env);
    files = [readOut(c.out), ""];
    rmSync(c.out, { force: true });
    rust = await runAsync(rustArgs, env);
    files[1] = readOut(c.out);
    rmSync(c.out, { force: true });
  } else {
    [ts, rust] = await Promise.all([
      runAsync(tsArgs, env),
      runAsync(rustArgs, env),
    ]);
  }
  stats.cases += 1;
  if (DEFECT.test(ts.stdout)) {
    stats.defects += 1;
    return;
  }
  if (live?.()) {
    stats.live += 1;
    return;
  }
  const a = masked(ts);
  const b = masked(rust);
  if (a.stdout !== b.stdout && c.args.includes("--json")) {
    report(`${label}: json${jsonHint(a.stdout, b.stdout)}`);
  }
  compareRun(label, a, b);
  if (files[0] !== files[1]) {
    report(`${label}: file ${firstLineDiff(files[0] ?? "", files[1] ?? "")}`);
  }
};

const listRows = async (env: Record<string, string>) => {
  const out = await runAsync(
    [rustBin, "--json", "sessions", "ls", "--agent", "claude", "--limit", "0"],
    env
  );
  return JSON.parse(out.stdout) as Row[];
};

/** `id` up to its first `-`, when no other listed id starts with it. */
const shortIdOf = (id: string, ids: readonly string[]) => {
  const short = id.split("-")[0] ?? id;
  return short !== id && ids.filter((x) => x.startsWith(short)).length === 1
    ? short
    : undefined;
};

const analyze = (t: string, ...rest: string[]) => [
  "sessions",
  "analyze",
  t,
  ...rest,
];
const exportOf = (t: string, ...rest: string[]) => [
  "sessions",
  "export",
  t,
  ...rest,
];

/** Every mode for a full id; a smaller set for its other names. An id that
 * starts with `-` reads as a flag in both CLIs, so only its path is used. */
const targetCases = (row: Row, ids: readonly string[]): Case[] => {
  const byPath: Case[] = [
    { args: analyze(row.path, "--json") },
    { args: exportOf(row.path, "--format", "html") },
  ];
  if (row.id.startsWith("-")) {
    return byPath;
  }
  const all: Case[] = [
    { args: analyze(row.id, "--json") },
    { args: analyze(row.id) },
    { args: ["--pretty", ...analyze(row.id)] },
    { args: exportOf(row.id, "--format", "json") },
    { args: exportOf(row.id, "--format", "md") },
    { args: exportOf(row.id, "--format", "html") },
    { args: exportOf(row.id, "--no-redact", "--format", "json") },
    { args: exportOf(row.id) },
  ];
  const short = shortIdOf(row.id, ids);
  return [
    ...all,
    ...(short
      ? [
          { args: analyze(short, "--json") },
          { args: exportOf(short, "--format", "md") },
        ]
      : []),
    ...byPath,
  ];
};

/** `--parent` cases for the first subagents of a session. */
const subagentCases = async (row: Row, env: Record<string, string>) => {
  const out = await runAsync(
    [rustBin, "sessions", "analyze", row.id, "--json"],
    env
  );
  let subs: { id: string }[] = [];
  try {
    subs = (JSON.parse(out.stdout) as { subagents: { id: string }[] })
      .subagents;
  } catch {
    return [];
  }
  return subs
    .slice(0, SUBAGENTS_PER_SESSION)
    .flatMap((s) => [
      { args: analyze(s.id, "--parent", row.id, "--json") },
      { args: exportOf(s.id, "--parent", row.id, "--format", "md") },
    ]);
};

/** Target, flag and write errors, run once per fixture suite. */
const fixedCases = (rows: readonly Row[], work: string): Case[] => {
  const first = rows[0]?.id ?? "x";
  const ids = rows.map((r) => r.id);
  const shared = ids
    .map((id) => id.slice(0, 1))
    .find((p) => ids.filter((x) => x.startsWith(p)).length > 1);
  return [
    { args: ["sessions", "analyze", "--latest"] },
    { args: ["sessions", "analyze", "--latest", "--json"] },
    { args: ["sessions", "analyze", "--cwd", "/"] },
    { args: ["sessions", "analyze", "--cwd", "/Users/demo/proj"] },
    { args: ["sessions", "analyze", "--cwd", "/nope"] },
    { args: ["sessions", "analyze"] },
    { args: ["sessions", "analyze", "x", "--latest"] },
    { args: ["sessions", "analyze", "--parent", "p"] },
    { args: ["sessions", "analyze", "--cwd"] },
    { args: ["sessions", "export", "--out"] },
    { args: ["sessions", "analyze", "-x"] },
    { args: ["sessions", "analyze", "zzz"] },
    { args: ["sessions", "analyze", "zzz.jsonl"] },
    ...(shared ? [{ args: ["sessions", "analyze", shared] }] : []),
    { args: ["sessions", "analyze", first, "--parent", "zz"] },
    { args: ["sessions", "analyze", "agent-1", "--parent", first] },
    { args: ["sessions", "export", first, "--format", "pdf"] },
    { args: ["sessions", "export", "--latest", "--format", "json"] },
    {
      args: ["sessions", "export", first, "--out", join(work, "x.HTM")],
      out: join(work, "x.HTM"),
    },
    {
      args: [
        "sessions",
        "export",
        first,
        "--out",
        join(work, "x.json"),
        "--no-redact",
      ],
      out: join(work, "x.json"),
    },
    {
      args: [
        "sessions",
        "export",
        first,
        "--out",
        join(work, "missing", "x.md"),
      ],
    },
    { args: ["sessions", "export", first, "--out", work] },
    { args: ["sessions", "export", first, "--out", "-"] },
    { args: ["--json", "sessions", "export", first] },
  ];
};

/** A suite in full, or (`zoneOnly`) just the dates a time zone changes. */
const runSuite = async (suite: Suite, zoneOnly: boolean) => {
  const rows = await listRows(suite.env);
  const ids = rows.map((r) => r.id);
  const work = join(WORK, `out-${suite.name}`);
  mkdirSync(work, { recursive: true });
  const subs = zoneOnly
    ? []
    : (await pool(rows, (r) => subagentCases(r, suite.env))).flat();
  const cases = zoneOnly
    ? rows.map((r) => ({ args: analyze(r.path, "--json") }))
    : [...rows.flatMap((r) => targetCases(r, ids)), ...subs];
  console.log(
    `suite ${suite.name} analyze: ${rows.length} sessions, ${cases.length} cases`
  );
  await pool(cases, (c) => runCase(suite.name, suite.env, c));
  if (!zoneOnly) {
    for (const c of fixedCases(rows, work)) {
      await runCase(suite.name, suite.env, c);
    }
  }
};

/** Stamp of a transcript and every file under its session dir. */
const stampOf = (path: string) => {
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
  walk(path);
  walk(path.replace(JSONL, ""));
  return parts.join("|");
};

/** Sessions to compare on the real corpus. */
const sampleRows = (rows: readonly Row[], all: boolean) => {
  if (all) {
    return [...rows];
  }
  const withSubagents = rows.filter((r) =>
    existsSync(join(dirname(r.path), r.id, "subagents"))
  );
  const compacted = rows.filter((r) => {
    try {
      return readFileSync(r.path, "utf8").includes('"compact_boundary"');
    } catch {
      return false;
    }
  });
  const picked = [
    ...rows.slice(0, SAMPLE.newest),
    ...[...rows]
      .sort((a, b) => b.sizeBytes - a.sizeBytes)
      .slice(0, SAMPLE.largest),
    ...withSubagents.slice(0, SAMPLE.subagents),
    ...compacted.slice(0, SAMPLE.compaction),
  ];
  return [...new Map(picked.map((r) => [r.path, r])).values()];
};

const runReal = async (pin: Record<string, string>, all: boolean) => {
  const env = { ...process.env, ...pin, PEEKTRACE_NO_TELEMETRY: "1" } as Record<
    string,
    string
  >;
  const rows = await listRows(env);
  const ids = rows.map((r) => r.id);
  const picked = sampleRows(rows, all);
  const before = new Map(picked.map((r) => [r.path, stampOf(r.path)]));
  const subs = (await pool(picked, (r) => subagentCases(r, env))).flat();
  const owner = (c: Case) =>
    picked.find((r) => c.args.includes(r.id) || c.args.includes(r.path));
  const cases = [...picked.flatMap((r) => targetCases(r, ids)), ...subs];
  console.log(
    `suite real analyze: ${picked.length} of ${rows.length} sessions, ${cases.length} cases`
  );
  await pool(cases, (c) =>
    runCase("real", env, c, () => {
      const r = owner(c);
      return r !== undefined && before.get(r.path) !== stampOf(r.path);
    })
  );
};

export const runAnalyzeStage = async (args: {
  readonly fixtures: readonly Suite[];
  readonly real: boolean;
  readonly realAll: boolean;
}) => {
  const empty = join(WORK, "empty-agents");
  mkdirSync(empty, { recursive: true });
  const pin = {
    PEEKTRACE_CODEX_SESSIONS: empty,
    PEEKTRACE_PI_SESSIONS: empty,
    PEEKTRACE_OPENCODE_DATA: empty,
  };
  if (args.fixtures.length > 0) {
    const home = join(WORK, "analyze-home");
    buildAnalyzeEdge(home);
    const suites: Suite[] = [
      ...args.fixtures.map((s) => ({ ...s, env: { ...s.env, ...pin } })),
      { name: "analyze-edge", args: [], env: fixtureEnv(home, pin) },
    ];
    try {
      for (const s of suites) {
        await runSuite(s, s.env.TZ !== "UTC");
      }
      await runSuite(
        {
          name: "analyze-edge-tokyo",
          args: [],
          env: fixtureEnv(home, { ...pin, TZ: "Asia/Tokyo" }),
        },
        true
      );
    } finally {
      for (const rel of LOCKED) {
        chmodSync(join(home, rel), READ_WRITE);
      }
    }
  }
  if (args.real) {
    await runReal(pin, args.realAll);
  }
  console.log(
    `analyze stage: ${stats.cases} cases, ${stats.defects} skipped as TS defects, ${stats.live} skipped as live`
  );
};
