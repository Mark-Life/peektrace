/**
 * `stats` parity stage: report, drill, prompt, usage and refresh, run by TS
 * then by Rust with one shared `PEEKTRACE_DIR` (TS caches in `stats/`, Rust
 * in `stats-rs/`, both read and write the same `rule-lines.json`).
 *
 * Normalisation, only this:
 * - JSON report and usage: `generatedAt`, `window.fromIso` and
 *   `window.toIso` are dropped (both runs read their own clock).
 * - Text report: line 1 carries the window bounds and is masked.
 * - Real suite: a JSON or text mismatch is run again once on both sides (a
 *   fact at the window edge, or a block's `active` flag, can move between
 *   the two runs).
 * - Cases where TS fails with an Effect defect are skipped, not diffed.
 */
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
  compareRun,
  fixtureEnv,
  REPO,
  type Run,
  report,
  run,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { buildStatsHome, unlockStatsHome } from "./parity-stats-fixtures";

type Args = readonly string[];

const TS_DEFECT =
  /^\[\d\d:\d\d:\d\d\.\d{3}\] ERROR \(#|Could not scan the corpus: TypeError/m;
const WINDOW_LINE = /^peektrace stats {2}(\S+) window {2}.* → .*$/m;
const BASE = ["--no-telemetry"];
const GENERATED_AT = /^ {2}"generatedAt": .*\n/m;
const FROM_ISO = /^ {4}"fromIso": .*\n/m;
const TO_ISO = /^ {4}"toIso": .*\n/m;
const CAUSE_PREFIX = /^cause:/;

/** Drop the clock-dependent fields of a report or usage JSON body. */
const stripClock = (stdout: string): string => {
  try {
    const v = JSON.parse(stdout) as Record<string, unknown>;
    if (typeof v !== "object" || v === null || !("schemaVersion" in v)) {
      return stdout;
    }
    return stdout
      .replace(GENERATED_AT, "")
      .replace(FROM_ISO, "")
      .replace(TO_ISO, "");
  } catch {
    return stdout;
  }
};

const normalise = (r: Run): Run => ({
  ...r,
  stdout: stripClock(r.stdout).replace(
    WINDOW_LINE,
    "peektrace stats  $1 window  <window>"
  ),
});

interface Stage {
  readonly env: Record<string, string>;
  readonly name: string;
  readonly retry: boolean;
}

const runBoth = (stage: Stage, args: Args) => {
  const ts = run([...TS_CMD, ...BASE, ...args], stage.env);
  const rust = run([rustBin, ...BASE, ...args], stage.env);
  return { ts: normalise(ts), rust: normalise(rust) };
};

const same = (a: Run, b: Run) =>
  a.code === b.code && a.stdout === b.stdout && a.stderr === b.stderr;

let checked = 0;

/** Run one case; returns the TS stdout for key discovery. */
const check = (stage: Stage, args: Args): string => {
  checked += 1;
  let { ts, rust } = runBoth(stage, args);
  if (TS_DEFECT.test(ts.stdout) || TS_DEFECT.test(ts.stderr)) {
    console.log(`  skip (TS defect) [${args.join(" ")}]`);
    return "";
  }
  if (stage.retry && !same(ts, rust)) {
    ({ ts, rust } = runBoth(stage, args));
  }
  compareRun(`${stage.name} [${args.join(" ")}]`, ts, rust);
  return ts.stdout;
};

interface Keys {
  readonly causes: string[];
  readonly clusters: string[];
  readonly fails: string[];
  readonly time: string[];
}

const keysOf = (stdout: string, cap: number): Keys => {
  try {
    const r = JSON.parse(stdout) as Record<string, { key: string }[]>;
    const take = (rows: { key: string }[] | undefined) =>
      (rows ?? []).map((row) => row.key).slice(0, cap);
    return {
      causes: take(r.causes),
      fails: take(r.fails),
      time: take(r.time),
      clusters: take(r.clusters),
    };
  } catch {
    return { causes: [], fails: [], time: [], clusters: [] };
  }
};

const runCases = (stage: Stage, days: string, cap: number) => {
  console.log(`suite stats-${stage.name}`);
  const before = checked;
  const scope = ["--agent", "claude", "--days", days];
  check(stage, ["stats", "refresh", "--agent", "claude", "--force", "--json"]);
  check(stage, ["stats", "refresh", "--agent", "claude", "--json"]);
  const keys = keysOf(check(stage, ["stats", ...scope, "--json"]), cap);
  check(stage, ["stats", ...scope]);
  check(stage, ["--pretty", "stats", ...scope]);
  check(stage, ["stats", ...scope, "--limit", "1", "--json"]);
  check(stage, ["stats", ...scope, "--limit", "0", "--json"]);
  check(stage, ["stats", "--agent", "claude", "--days", "0", "--json"]);
  const drill = (flag: string, key: string) =>
    check(stage, ["stats", "drill", `--${flag}`, key, ...scope, "--json"]);
  for (const key of keys.causes) {
    drill("cause", key);
    drill("cause", key.replace(CAUSE_PREFIX, ""));
  }
  for (const k of keys.fails) {
    drill("fail", k);
  }
  for (const k of keys.time) {
    drill("time", k);
  }
  for (const k of keys.clusters) {
    drill("cluster", k);
  }
  for (const k of ["all", "wait/poll", "bare-delay", "loop-poll"]) {
    drill("wait", k);
  }
  const first = keys.causes[0];
  if (first !== undefined) {
    check(stage, ["stats", "drill", "--cause", first, ...scope]);
    check(stage, ["--pretty", "stats", "drill", "--cause", first, ...scope]);
    check(stage, ["stats", "prompt", "--cause", first, ...scope]);
    check(stage, [
      "stats",
      "prompt",
      "--cause",
      first.replace(CAUSE_PREFIX, ""),
      ...scope,
    ]);
  }
  for (const k of keys.fails.slice(0, 1)) {
    check(stage, ["stats", "prompt", "--fail", k, ...scope]);
  }
  for (const k of keys.time.slice(0, 1)) {
    check(stage, ["stats", "prompt", "--time", k, ...scope]);
  }
  check(stage, ["stats", "prompt", ...scope]);
  check(stage, ["stats", "prompt", "--cause", "nope", ...scope]);
  check(stage, ["stats", "prompt", "--cause", "a", "--time", "b", ...scope]);
  check(stage, ["stats", "drill", "--cause", "nope", ...scope]);
  check(stage, ["stats", "drill", ...scope]);
  check(stage, ["stats", "drill", "--cause", "a", "--fail", "b", ...scope]);
  const usageDays = stage.name === "real" ? [] : ["--days", days];
  for (const by of ["day", "month", "block"]) {
    for (const tz of ["UTC", "America/New_York", "Asia/Kolkata", "+05:30"]) {
      check(stage, [
        "stats",
        "usage",
        "--by",
        by,
        "--tz",
        tz,
        "--agent",
        "claude",
        ...(by === "block" ? [] : usageDays),
        "--json",
      ]);
    }
  }
  check(stage, [
    "stats",
    "usage",
    "--by",
    "block",
    "--tz",
    "UTC",
    "--agent",
    "claude",
  ]);
  check(stage, [
    "--pretty",
    "stats",
    "usage",
    "--by",
    "day",
    "--tz",
    "UTC",
    "--agent",
    "claude",
    ...usageDays,
  ]);
  check(stage, ["stats", "usage", "--tz", "Mars/Base", "--agent", "claude"]);
  check(stage, ["stats", "usage", "--days", "0", "--agent", "claude"]);
  check(stage, ["stats", "usage", "--days", "36501", "--agent", "claude"]);
  check(stage, ["stats", "usage", "--by", "week", "--agent", "claude"]);
  check(stage, ["stats", "--agent", "bogus"]);
  check(stage, ["stats", "--days", "abc", "--agent", "claude"]);
  check(stage, ["stats", "refresh", "--agent", "claude"]);
  check(stage, ["--pretty", "stats", "refresh", "--agent", "claude"]);
  console.log(`  ${checked - before} cases`);
};

/** Point Codex, Pi and OpenCode at an empty dir: Rust reads Claude only. */
const otherAgentsEmpty = (root: string) => {
  const empty = join(root, "empty");
  mkdirSync(empty, { recursive: true });
  return {
    PEEKTRACE_CODEX_SESSIONS: empty,
    PEEKTRACE_PI_SESSIONS: empty,
    PEEKTRACE_OPENCODE_DATA: empty,
  };
};

/** Snapshot `~/.claude/projects` so live sessions cannot move numbers. */
const snapshotReal = (dest: string): string | null => {
  const src = join(process.env.HOME ?? "", ".claude/projects");
  if (!existsSync(src)) {
    return null;
  }
  rmSync(dest, { recursive: true, force: true });
  const clone = Bun.spawnSync(
    process.platform === "darwin"
      ? ["cp", "-cpR", src, dest]
      : ["cp", "-a", src, dest]
  );
  if (clone.exitCode !== 0) {
    cpSync(src, dest, { recursive: true });
  }
  return dest;
};

/** The stats stage: fixture HOME, TS fixtures root, then the real corpus. */
export const runStatsStage = (opts: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  const root = join(WORK, "stats");
  mkdirSync(root, { recursive: true });
  const others = otherAgentsEmpty(root);
  if (opts.fixtures) {
    const home = buildStatsHome(root);
    const fx: Stage = {
      name: "fx",
      env: fixtureEnv(home, others),
      retry: false,
    };
    runCases(fx, "36500", 50);
    runCases(
      { ...fx, name: "fx-tokyo", env: { ...fx.env, TZ: "Asia/Tokyo" } },
      "36500",
      3
    );
    const tsFixtures = join(REPO, "packages/core/test/fixtures");
    const emptyHome = join(root, "empty-home");
    mkdirSync(emptyHome, { recursive: true });
    runCases(
      {
        name: "ts-fixtures",
        env: fixtureEnv(emptyHome, {
          ...others,
          PEEKTRACE_CLAUDE_PROJECTS: tsFixtures,
        }),
        retry: false,
      },
      "36500",
      50
    );
    unlockStatsHome(home);
  }
  if (opts.real) {
    const projects = snapshotReal(join(root, "real-projects"));
    if (projects === null) {
      console.log("suite stats-real: no ~/.claude/projects, skipped");
      return;
    }
    const env: Record<string, string> = {
      ...(process.env as Record<string, string>),
      ...others,
      PEEKTRACE_CLAUDE_PROJECTS: projects,
      PEEKTRACE_DIR: join(root, "real-pdir"),
      PEEKTRACE_NO_TELEMETRY: "1",
    };
    runCases({ name: "real", env, retry: true }, "60", 3);
  }
  if (!existsSync(rustBin)) {
    report("stats: rust binary missing");
  }
};
