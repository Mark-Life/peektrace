/**
 * `stats` parity stage: report, drill, prompt, usage and refresh, run by TS
 * then by Rust with one shared `PEEKTRACE_DIR` (TS caches in `stats/`, Rust
 * in `stats-rs/`, both read and write the same `rule-lines.json`). Each suite
 * runs every agent at once first, then one agent at a time, so both sides
 * walk the same index merge history.
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
 *
 * The `*nullrow*` Codex and Pi edge files stay out: a `null` line makes the
 * whole TS scan fail (a known difference), which would drop every case.
 */
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import {
  agentsSettings,
  buildAgentsEdge,
  buildPiRoot,
} from "./parity-agents-fixtures";
import { CODEX_NULLROW } from "./parity-codex-fixtures";
import { fixtureEnv, REPO, report, rustBin, WORK } from "./parity-lib";
import { buildOpencodeData } from "./parity-opencode-fixtures";
import { PI_NULLROW } from "./parity-pi-fixtures";
import { runCases, runErrors, type Stage } from "./parity-stats-cases";
import { buildStatsHome, unlockStatsHome } from "./parity-stats-fixtures";

const ALL_DAYS = "36500";
const REAL_DAYS = "60";
const FULL_CAP = 50;
const SMALL_CAP = 3;
const READ_WRITE = 0o644;
const OTHER_AGENTS = ["codex", "pi", "opencode"] as const;
const agent = (a: string) => ["--agent", a];

/** Every agent and Claude on the full matrix, the others reduced. */
const runScopes = (stage: Stage, days: string, cap: number) => {
  runCases(stage, days, cap, [], true);
  runCases(stage, days, cap, agent("claude"), true);
  for (const a of OTHER_AGENTS) {
    runCases(stage, days, cap, agent(a), false);
  }
  runErrors(stage);
};

/**
 * Codex and Pi edge trees plus the OpenCode edge DBs, minus the null-row
 * files; returns the env and the files to unlock afterwards.
 */
const buildAgentsEdgeHome = (root: string) => {
  const base = join(root, "agents-edge");
  rmSync(base, { recursive: true, force: true });
  const home = join(base, "home");
  mkdirSync(join(home, ".claude/projects"), { recursive: true });
  const extra = join(base, "extra");
  const locked = buildAgentsEdge(home, extra);
  rmSync(join(home, ".codex/sessions", CODEX_NULLROW));
  rmSync(join(home, ".pi/agent/sessions", PI_NULLROW));
  const oc = buildOpencodeData(
    join(base, "oc"),
    join(REPO, "packages/core/test/fixtures/sessions/projects")
  );
  oc.close();
  mkdirSync(join(home, ".peektrace"), { recursive: true });
  writeFileSync(
    join(home, ".peektrace/settings.json"),
    JSON.stringify({ roots: agentsSettings(extra, join(base, "missing")) })
  );
  return {
    env: fixtureEnv(home, { PEEKTRACE_OPENCODE_DATA: oc.main }),
    locked,
  };
};

const runFixtureSuites = (root: string) => {
  const home = buildStatsHome(root);
  const fx: Stage = { name: "fx", env: fixtureEnv(home), retry: false };
  runScopes(fx, ALL_DAYS, FULL_CAP);
  const tokyo = {
    ...fx,
    name: "fx-tokyo",
    env: { ...fx.env, TZ: "Asia/Tokyo" },
  };
  runCases(tokyo, ALL_DAYS, SMALL_CAP, [], true);
  runErrors(tokyo);
  const tsFixtures = join(REPO, "packages/core/test/fixtures");
  const emptyHome = join(root, "empty-home");
  const empty = join(root, "empty");
  rmSync(emptyHome, { recursive: true, force: true });
  mkdirSync(emptyHome, { recursive: true });
  mkdirSync(empty, { recursive: true });
  const piRoot = join(root, "ts-pi");
  const codexRoot = join(root, "ts-codex");
  rmSync(piRoot, { recursive: true, force: true });
  rmSync(codexRoot, { recursive: true, force: true });
  // A copy: the Claude root also lists `sessions-codex/`, and two refs with
  // one path share one cache entry, which each side fills in its own order.
  cpSync(join(tsFixtures, "sessions-codex"), codexRoot, { recursive: true });
  const ts: Stage = {
    name: "ts-fixtures",
    env: fixtureEnv(emptyHome, {
      PEEKTRACE_CLAUDE_PROJECTS: tsFixtures,
      PEEKTRACE_CODEX_SESSIONS: codexRoot,
      PEEKTRACE_PI_SESSIONS: buildPiRoot(tsFixtures, piRoot),
      PEEKTRACE_OPENCODE_DATA: empty,
    }),
    retry: false,
  };
  runCases(ts, ALL_DAYS, FULL_CAP, [], true);
  runErrors(ts);
  unlockStatsHome(home);
  const edge = buildAgentsEdgeHome(root);
  try {
    const stage: Stage = { name: "agents-edge", env: edge.env, retry: false };
    runCases(stage, ALL_DAYS, SMALL_CAP, [], true);
    for (const a of OTHER_AGENTS) {
      runCases(stage, ALL_DAYS, SMALL_CAP, agent(a), false);
    }
    runErrors(stage);
  } finally {
    for (const path of edge.locked) {
      chmodSync(path, READ_WRITE);
    }
  }
};

/**
 * Copy `src` to `dest` so live sessions cannot move numbers. On macOS
 * `cp -c` makes an APFS clone; elsewhere it is a full copy.
 */
const snapshot = (src: string, dest: string): boolean => {
  if (!existsSync(src)) {
    return false;
  }
  rmSync(dest, { recursive: true, force: true });
  const size = Bun.spawnSync(["du", "-sh", src])
    .stdout.toString()
    .split("\t")[0];
  const how = process.platform === "darwin" ? "cloning" : "copying";
  console.log(`suite stats-real: ${how} ${src} (${size || "?"}) to ${dest}`);
  const clone = Bun.spawnSync(
    process.platform === "darwin"
      ? ["cp", "-cpR", src, dest]
      : ["cp", "-a", src, dest]
  );
  if (clone.exitCode !== 0) {
    cpSync(src, dest, { recursive: true });
  }
  return true;
};

/** The OpenCode data dir: only its DB files and `storage/`. */
const snapshotOpencode = (src: string, dest: string) => {
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  if (!existsSync(src)) {
    return;
  }
  for (const name of readdirSync(src)) {
    if (name.startsWith("opencode") && name.includes(".db")) {
      snapshot(join(src, name), join(dest, name));
    }
  }
  snapshot(join(src, "storage"), join(dest, "storage"));
};

const opencodeDataDir = (home: string) => {
  const xdg = process.env.XDG_DATA_HOME;
  return join(xdg ? xdg : join(home, ".local/share"), "opencode");
};

/** The real suite on snapshots of every agent's sessions, deleted after.
 * Memory files stay real. */
const runRealCases = (root: string) => {
  const home = process.env.HOME ?? "";
  const dirs = {
    projects: join(root, "real-projects"),
    codex: join(root, "real-codex"),
    pi: join(root, "real-pi"),
    opencode: join(root, "real-opencode"),
    pdir: join(root, "real-pdir"),
  };
  if (!snapshot(join(home, ".claude/projects"), dirs.projects)) {
    console.log("suite stats-real: no ~/.claude/projects, skipped");
    return;
  }
  try {
    const codexHome = process.env.CODEX_HOME || join(home, ".codex");
    if (!snapshot(join(codexHome, "sessions"), dirs.codex)) {
      mkdirSync(dirs.codex, { recursive: true });
    }
    if (!snapshot(join(home, ".pi/agent/sessions"), dirs.pi)) {
      mkdirSync(dirs.pi, { recursive: true });
    }
    snapshotOpencode(opencodeDataDir(home), dirs.opencode);
    const stage: Stage = {
      name: "real",
      env: {
        ...(process.env as Record<string, string>),
        PEEKTRACE_CLAUDE_PROJECTS: dirs.projects,
        PEEKTRACE_CODEX_SESSIONS: dirs.codex,
        PEEKTRACE_PI_SESSIONS: dirs.pi,
        PEEKTRACE_OPENCODE_DATA: dirs.opencode,
        PEEKTRACE_DIR: dirs.pdir,
        PEEKTRACE_NO_TELEMETRY: "1",
      },
      retry: true,
    };
    runCases(stage, REAL_DAYS, SMALL_CAP, [], true);
    for (const a of OTHER_AGENTS) {
      runCases(stage, ALL_DAYS, SMALL_CAP, agent(a), false);
    }
  } finally {
    for (const dir of Object.values(dirs)) {
      rmSync(dir, { recursive: true, force: true });
    }
  }
};

/** The stats stage: fixture HOMEs and the TS fixtures, then the real corpus. */
export const runStatsStage = (opts: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  const root = join(WORK, "stats");
  mkdirSync(root, { recursive: true });
  if (opts.fixtures) {
    runFixtureSuites(root);
  }
  if (opts.real) {
    runRealCases(root);
  }
  if (!existsSync(rustBin)) {
    report("stats: rust binary missing");
  }
};
