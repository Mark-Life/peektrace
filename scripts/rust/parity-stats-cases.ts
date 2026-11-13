/**
 * The `stats` parity cases: each one runs TS then Rust with the same env and
 * diffs exit code, stdout and stderr after the normalisation below.
 *
 * Normalisation, only this:
 * - JSON report and usage: `generatedAt`, `window.fromIso` and
 *   `window.toIso` are dropped (both runs read their own clock).
 * - Text report: line 1 carries the window bounds and is masked.
 * - Real suite: a JSON or text mismatch is run again once on both sides (a
 *   fact at the window edge, or a block's `active` flag, can move between
 *   the two runs).
 * - Cases where TS fails with an Effect defect are skipped, not diffed, but
 *   counted; Rust must still exit 0 or 1 there. A skipped key-discovery
 *   case fails the stage, since every keyed drill and prompt case drops.
 */
import {
  compareRun,
  type Run,
  report,
  run,
  rustBin,
  TS_CMD,
} from "./parity-lib";

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

/** A stats text or JSON body with its clock-dependent parts masked. */
export const maskStatsClock = (text: string) =>
  stripClock(text).replace(WINDOW_LINE, "peektrace stats  $1 window  <window>");

const normalise = (r: Run): Run => ({ ...r, stdout: maskStatsClock(r.stdout) });

const CLOCK_VALUES = [
  /^( {2}"generatedAt"): .*?(,?)$/m,
  /^( {4}"fromIso"): .*?(,?)$/m,
  /^( {4}"toIso"): .*?(,?)$/m,
];

/** Record-time canon: the clock fields keep their lines with a fixed value,
 * so the JSON still parses for key discovery and `maskStatsClock` drops
 * them as before. */
export const canonStatsClock = (r: Run): Run => {
  try {
    const v = JSON.parse(r.stdout) as Record<string, unknown>;
    if (typeof v !== "object" || v === null || !("schemaVersion" in v)) {
      return r;
    }
    const stdout = CLOCK_VALUES.reduce(
      (acc, re) => acc.replace(re, '$1: "<clock>"$2'),
      r.stdout
    );
    return { ...r, stdout };
  } catch {
    return { ...r, stdout: maskStatsClock(r.stdout) };
  }
};

export interface Stage {
  readonly env: Record<string, string>;
  readonly name: string;
  readonly retry: boolean;
}

const runBoth = (stage: Stage, args: Args) => {
  const ts = run([...TS_CMD, ...BASE, ...args], stage.env, {
    canon: canonStatsClock,
  });
  const rust = run([rustBin, ...BASE, ...args], stage.env);
  return { raw: ts.stdout, ts: normalise(ts), rust: normalise(rust) };
};

const same = (a: Run, b: Run) =>
  a.code === b.code && a.stdout === b.stdout && a.stderr === b.stderr;

let checked = 0;
let skipped = 0;

/** Run one case; returns the raw TS stdout for key discovery, or null on a TS defect. */
const check = (stage: Stage, args: Args): string | null => {
  checked += 1;
  const label = `${stage.name} [${args.join(" ")}]`;
  let { raw, ts, rust } = runBoth(stage, args);
  if (TS_DEFECT.test(ts.stdout) || TS_DEFECT.test(ts.stderr)) {
    skipped += 1;
    console.log(`  skip (TS defect) [${args.join(" ")}]`);
    if (rust.code !== 0 && rust.code !== 1) {
      report(`${label}: TS defect, rust exit ${rust.code}`);
    }
    return null;
  }
  if (stage.retry && !same(ts, rust)) {
    ({ raw, ts, rust } = runBoth(stage, args));
  }
  compareRun(label, ts, rust);
  return raw;
};

interface Keys {
  readonly causes: string[];
  readonly clusters: string[];
  readonly fails: string[];
  readonly time: string[];
}

const NO_KEYS: Keys = { causes: [], fails: [], time: [], clusters: [] };

/** Drill and prompt keys from the raw TS report; a stage without them fails. */
const keysOf = (stage: Stage, stdout: string | null, cap: number): Keys => {
  if (stdout === null) {
    report(
      `${stage.name}: key discovery skipped (TS defect), keyed cases dropped`
    );
    return NO_KEYS;
  }
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
  } catch (e) {
    report(`${stage.name}: key discovery output is not JSON: ${String(e)}`);
    return NO_KEYS;
  }
};

/** `["--agent", a]`, or `[]` for every agent. */
type Scope = readonly string[];

const scopeName = (scope: Scope) => scope[1] ?? "all";

interface Matrix {
  readonly agent: Scope;
  readonly days: string;
  readonly full: boolean;
  readonly stage: Stage;
}

const scopeOf = (m: Matrix) => [...m.agent, "--days", m.days];

const reportCases = (m: Matrix) => {
  const { stage, agent, full } = m;
  const scope = scopeOf(m);
  check(stage, ["stats", ...scope]);
  check(stage, ["--pretty", "stats", ...scope]);
  check(stage, ["stats", ...scope, "--limit", "1", "--json"]);
  if (full) {
    check(stage, ["stats", ...scope, "--limit", "0", "--json"]);
    check(stage, ["stats", ...agent, "--days", "0", "--json"]);
  }
};

const drillCases = (m: Matrix, keys: Keys) => {
  const drill = (flag: string, key: string) =>
    check(m.stage, [
      "stats",
      "drill",
      `--${flag}`,
      key,
      ...scopeOf(m),
      "--json",
    ]);
  for (const key of keys.causes) {
    drill("cause", key);
    if (m.full) {
      drill("cause", key.replace(CAUSE_PREFIX, ""));
    }
  }
  for (const [flag, list] of [
    ["fail", keys.fails],
    ["time", keys.time],
    ["cluster", keys.clusters],
  ] as const) {
    for (const k of list) {
      drill(flag, k);
    }
  }
  const waits = m.full
    ? ["all", "wait/poll", "bare-delay", "loop-poll"]
    : ["all"];
  for (const k of waits) {
    drill("wait", k);
  }
};

const promptCases = (m: Matrix, keys: Keys) => {
  const { stage } = m;
  const scope = scopeOf(m);
  const first = keys.causes[0];
  if (first !== undefined) {
    check(stage, ["stats", "prompt", "--cause", first, ...scope]);
  }
  if (first !== undefined && m.full) {
    const bare = first.replace(CAUSE_PREFIX, "");
    check(stage, ["stats", "drill", "--cause", first, ...scope]);
    check(stage, ["--pretty", "stats", "drill", "--cause", first, ...scope]);
    check(stage, ["stats", "prompt", "--cause", bare, ...scope]);
  }
  for (const k of keys.fails.slice(0, 1)) {
    check(stage, ["stats", "prompt", "--fail", k, ...scope]);
  }
  for (const k of keys.time.slice(0, 1)) {
    check(stage, ["stats", "prompt", "--time", k, ...scope]);
  }
  check(stage, ["stats", "prompt", ...scope]);
  if (m.full) {
    check(stage, ["stats", "prompt", "--cause", "nope", ...scope]);
    check(stage, ["stats", "drill", "--cause", "nope", ...scope]);
  }
};

const FULL_ZONES = ["UTC", "America/New_York", "Asia/Kolkata", "+05:30"];
const SHORT_ZONES = ["UTC", "Asia/Kolkata"];
const ENV_ZONES = ["Asia/Kolkata", "US/Pacific", "Etc/UTC", "Europe/Kyiv"];

const usageCases = ({ stage, agent, days, full }: Matrix) => {
  const usageDays = stage.name === "real" ? [] : ["--days", days];
  const usage = ["stats", "usage", ...agent];
  for (const by of ["day", "month"]) {
    for (const tz of full ? FULL_ZONES : SHORT_ZONES) {
      check(stage, [...usage, "--by", by, "--tz", tz, ...usageDays, "--json"]);
    }
  }
  for (const tz of full ? FULL_ZONES : ["UTC"]) {
    check(stage, [...usage, "--by", "block", "--tz", tz, "--json"]);
  }
  check(stage, [...usage, "--tz", "UTC", ...usageDays]);
  if (!full) {
    return;
  }
  check(stage, [...usage, "--by", "block", "--tz", "UTC"]);
  check(stage, [
    "--pretty",
    ...usage,
    "--by",
    "day",
    "--tz",
    "UTC",
    ...usageDays,
  ]);
  for (const tz of ENV_ZONES) {
    const env = { ...stage.env, TZ: tz };
    check({ ...stage, env }, [...usage, ...usageDays, "--json"]);
    check({ ...stage, env }, [...usage, ...usageDays]);
  }
};

/**
 * One scope's cases. The full matrix covers every flag; the reduced one
 * keeps refresh, report, every drill key, prompts and usage per period.
 */
export const runCases = (
  stage: Stage,
  days: string,
  cap: number,
  agent: Scope,
  full: boolean
) => {
  console.log(`suite stats-${stage.name} (${scopeName(agent)})`);
  const before = checked;
  const skippedBefore = skipped;
  const m: Matrix = { stage, agent, days, full };
  check(stage, ["stats", "refresh", ...agent, "--force", "--json"]);
  check(stage, ["stats", "refresh", ...agent, "--json"]);
  const keys = keysOf(
    stage,
    check(stage, ["stats", ...scopeOf(m), "--json"]),
    cap
  );
  reportCases(m);
  drillCases(m, keys);
  promptCases(m, keys);
  usageCases(m);
  if (full) {
    check(stage, ["stats", "refresh", ...agent]);
    check(stage, ["--pretty", "stats", "refresh", ...agent]);
  }
  const skips = skipped - skippedBefore;
  console.log(
    `  ${checked - before} cases${skips > 0 ? `, ${skips} skipped (TS defect)` : ""}`
  );
};

/** Flag errors, once per suite: they do not depend on the corpus. */
export const runErrors = (stage: Stage) => {
  console.log(`suite stats-${stage.name} (errors)`);
  const before = checked;
  for (const args of [
    ["stats", "prompt", "--cause", "a", "--time", "b"],
    ["stats", "drill"],
    ["stats", "drill", "--cause", "a", "--fail", "b"],
    ["stats", "usage", "--tz", "Mars/Base"],
    ["stats", "usage", "--days", "0"],
    ["stats", "usage", "--days", "36501"],
    ["stats", "usage", "--by", "week"],
    ["stats", "--agent", "bogus"],
    ["stats", "--days", "abc"],
  ]) {
    check(stage, args);
  }
  console.log(`  ${checked - before} cases`);
};
