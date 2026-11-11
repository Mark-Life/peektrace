/**
 * Analyze/export stage of the parity harness. Every listed Claude, Codex and
 * Pi session runs through `sessions analyze` and `sessions export` in every
 * output mode, addressed by full id, short id and path, plus Claude
 * subagents via `--parent` and a fixed set of target and flag errors.
 *
 * Normalisation:
 * - The OpenCode root points at an empty dir, so TS prefix lookups and
 *   `--latest` see the same sessions as the Rust CLI. `parity-opencode.ts`
 *   runs the OpenCode cases.
 * - `exportedAt` (JSON), the `Exported:` line (md) and `<dt>Exported</dt>`
 *   (html) are the only time-based values; their stamp is masked.
 * - A TS defect (an Effect stack on stdout, e.g. a `null` row) skips the
 *   comparison, but Rust must still exit 0 with clean, parseable output. In
 *   fixture suites only the `KNOWN_DEFECTS` sessions may hit one, and each of
 *   them must.
 * - Real corpus: a sample by default (newest, largest, with subagents, with
 *   compaction, plus up to REAL_PER_AGENT newest Codex and Pi sessions);
 *   `--real-all` runs every session. Cases whose transcript or
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
import { basename, dirname, join } from "node:path";
import {
  analyze,
  type Case,
  fixedCases,
  type Row,
  subagentCases,
  targetCases,
} from "./parity-analyze-cases";
import { buildAnalyzeEdge, LOCKED } from "./parity-analyze-fixtures";
import {
  compareRun,
  DEFECT,
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
const JSONL = /\.jsonl$/;
const STAMP = /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/;
const SAMPLE = { newest: 30, largest: 10, subagents: 20, compaction: 10 };
const REAL_PER_AGENT = 100;
/** Fixture sessions the TS CLI fails on; see "Known differences" in the plan. */
const KNOWN_DEFECTS = new Set([
  "nonobj",
  "nullrow",
  "rollout-nullrow",
  "pi-nullrow",
  "ses_far",
]);
const REAL = "real";
const STDERR_SHOWN = 200;

const stats = { cases: 0, defects: 0, live: 0 };
const defectsSeen = new Set<string>();

const isJsonOut = (args: readonly string[]) =>
  args.includes("--json") ||
  args.some((a, i) => a === "--format" && args[i + 1] === "json");

/** The known-defect session a case targets, by id, short id or transcript
 * path. */
const knownDefect = (args: readonly string[]) =>
  args
    .filter((a) => !a.startsWith("-"))
    .map((a) => basename(a, ".jsonl"))
    .flatMap((a) => [...KNOWN_DEFECTS].filter((k) => k.startsWith(a)))
    .at(0);

/** TS failed, so there is nothing to compare: Rust must still succeed. */
const checkDefect = (suite: string, label: string, c: Case, rust: Run) => {
  stats.defects += 1;
  const known = knownDefect(c.args);
  if (known) {
    defectsSeen.add(known);
  } else if (suite !== REAL) {
    report(`${label}: unexpected TS defect`);
  }
  if (rust.code !== 0 || rust.stderr !== "") {
    report(
      `${label}: on TS defect rust exit=${rust.code} stderr=${JSON.stringify(rust.stderr.slice(0, STDERR_SHOWN))}`
    );
  }
  if (rust.stdout === "" || DEFECT.test(rust.stdout)) {
    report(`${label}: on TS defect rust stdout is empty or a defect`);
  }
  if (isJsonOut(c.args)) {
    try {
      JSON.parse(rust.stdout);
    } catch {
      report(`${label}: on TS defect rust --json output does not parse`);
    }
  }
};

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
export const runCase = async (
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
    checkDefect(suite, label, c, rust);
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
    [rustBin, "--json", "sessions", "ls", "--limit", "0"],
    env
  );
  return JSON.parse(out.stdout) as Row[];
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
  const withSubagents = rows.filter(
    (r) =>
      r.agent === "claude" &&
      existsSync(join(dirname(r.path), r.id, "subagents"))
  );
  const compacted = rows.filter((r) => {
    try {
      return readFileSync(r.path, "utf8").includes('"compact_boundary"');
    } catch {
      return false;
    }
  });
  const newestOf = (agent: string) =>
    rows.filter((r) => r.agent === agent).slice(0, REAL_PER_AGENT);
  const picked = [
    ...rows.slice(0, SAMPLE.newest),
    ...newestOf("codex"),
    ...newestOf("pi"),
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
  const owner = (c: Case) => picked.find((r) => r.path === c.owner);
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
  const pin = { PEEKTRACE_OPENCODE_DATA: empty };
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
      for (const name of KNOWN_DEFECTS) {
        if (!defectsSeen.has(name)) {
          report(`known TS defect ${name} no longer fails: compare it`);
        }
      }
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
