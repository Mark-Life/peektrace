/** Serve-stage feature cases: search, stats and memory over TS `/rpc` and
 * Rust `/api`.
 *
 * Fixtures: the stats fixture HOME with the memory edge vaults copied in.
 * - `features`: one HOME and one `PEEKTRACE_DIR` for both servers; search,
 *   stats and memory reads, TS call first, never in parallel.
 * - `features-ro`: `--read-only`; writes refused, stats allowed.
 * - `memory-writes`: twin HOME copies, one write sequence, trees diffed.
 *
 * Real corpus: reads (and stats, when on) on a snapshot of every agent's
 * sessions, so live sessions cannot move numbers; memory writes on twin
 * copies of the real `memory` dirs only, never on the real HOME.
 */
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { COPY, fixtureEnv, WORK } from "./parity-lib";
import { buildMemHome, EXTRA_SLUGS } from "./parity-memory-fixtures";
import type { Counts } from "./parity-serve-cases";
import { compareCall, type Norm, query } from "./parity-serve-compare";
import { type Pair, rpc, withPair } from "./parity-serve-lib";
import {
  memoryReadCases,
  memoryWriteCases,
  readOnlyFeatureCases,
  realMemoryWrites,
} from "./parity-serve-memory";
import { realStatsCases, statsCases } from "./parity-serve-stats";
import { opencodeDataDir, snapshot, snapshotOpencode } from "./parity-stats";
import { buildStatsHome, unlockStatsHome } from "./parity-stats-fixtures";

type Search = Record<string, string | number | boolean>;

const SEARCHES: readonly Search[] = [
  { q: "refactor the AUTH module" },
  { q: "no-such-phrase-xyz" },
  { q: "the" },
  { q: "the", limit: 1 },
  { q: "the", limit: 0 },
  { q: "the", since: "2020-01-01" },
  { q: "the", agent: "codex" },
  { q: "the", project: "-Users-demo-proj" },
  { q: "the", project: "-~-proj" },
  { q: "the", project: "" },
  { q: "the", redact: false, limit: 2 },
  { q: "  " },
  { q: "x", limit: -1 },
  { q: "x", limit: 1.5 },
  { q: "x", since: "7d" },
  { q: "the", agent: "all" },
];
const REAL_QUERIES = ["parity", "zsh", "peektrace serve"];
const REAL_NEWEST = 3;

/** A truncated scan stops at a race between loads: its counts vary. */
const searchNorm: Norm = (v) => {
  const r = v as { truncated?: boolean } & Record<string, unknown>;
  if (r?.truncated !== true) {
    return v;
  }
  const { scannedSessions: _s, unreadable: _u, ...rest } = r;
  return rest;
};

const searchCase = (c: Counts, pair: Pair, label: string, s: Search) =>
  compareCall(
    c,
    pair,
    `${label} sessions.search ${JSON.stringify(s)}`,
    {
      tag: "sessions.search",
      payload: s,
      path: `/api/sessions/search${query(s)}`,
    },
    { norm: searchNorm }
  );

/** Copy the memory edge vaults (and the `outside` dir) into `home`; the
 * unreadable file is left out. */
const copyMemory = (mem: string, home: string) => {
  const keep = (s: string) => !s.endsWith("locked.md");
  cpSync(join(mem, ".claude/projects"), join(home, ".claude/projects"), {
    ...COPY,
    filter: keep,
  });
  cpSync(join(mem, "outside"), join(home, "outside"), COPY);
};

export const fixtureFeatures = async (c: Counts, dist: string) => {
  const root = join(WORK, "serve", "features");
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  const home = buildStatsHome(root);
  const mem = join(root, "mem-home");
  buildMemHome(mem);
  copyMemory(mem, home);
  const tmp = join(root, "tmp");
  mkdirSync(tmp, { recursive: true });
  const env = fixtureEnv(home, { TMPDIR: tmp, PEEKTRACE_CLIENT_DIR: dist });
  try {
    await withPair(
      "features",
      env,
      [],
      async (pair) => {
        for (const s of SEARCHES) {
          await searchCase(c, pair, "fixture", s);
        }
        await statsCases(c, pair);
        await memoryReadCases(c, pair, "fixture", EXTRA_SLUGS);
      },
      join(root, "pdir")
    );
    await withPair("features-ro", env, ["--read-only"], (pair) =>
      readOnlyFeatureCases(c, pair)
    );
  } finally {
    unlockStatsHome(home);
  }
  const homes = { ts: join(root, "ts-home"), rust: join(root, "rust-home") };
  for (const h of Object.values(homes)) {
    copyMemory(mem, h);
  }
  await withPair(
    "memory-writes",
    (side) =>
      fixtureEnv(homes[side], { TMPDIR: tmp, PEEKTRACE_CLIENT_DIR: dist }),
    [],
    (pair) => memoryWriteCases(c, pair, homes)
  );
};

/** Twin HOMEs holding a copy of every real project's `memory` dir (and an
 * empty dir for each project); returns the first project with an index. */
const realMemoryHomes = (projects: string, homes: readonly string[]) => {
  const slugs = readdirSync(projects, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort();
  for (const home of homes) {
    for (const slug of slugs) {
      const dest = join(home, ".claude/projects", slug);
      mkdirSync(dest, { recursive: true });
      const memory = join(projects, slug, "memory");
      if (existsSync(memory)) {
        cpSync(memory, join(dest, "memory"), { recursive: true });
      }
    }
  }
  return slugs.find((s) => existsSync(join(projects, s, "memory/MEMORY.md")));
};

const newestSessions = async (pair: Pair) => {
  const listing = await rpc(pair.ts.port, "sessions.listing", {});
  const rows =
    listing.kind === "ok"
      ? (listing.value as { headers: { id: string; updatedAt: string }[] })
          .headers
      : [];
  return [...rows]
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    .slice(0, REAL_NEWEST)
    .map((r) => r.id);
};

export const realFeatures = async (
  c: Counts,
  dist: string,
  realStats: boolean
) => {
  const home = process.env.HOME ?? "";
  const projects = join(home, ".claude/projects");
  if (!existsSync(projects)) {
    return;
  }
  const root = join(WORK, "serve", "real-features");
  rmSync(root, { recursive: true, force: true });
  const dirs = {
    projects: join(root, "projects"),
    codex: join(root, "codex"),
    pi: join(root, "pi"),
    opencode: join(root, "opencode"),
  };
  try {
    snapshot(projects, dirs.projects);
    const codexHome = process.env.CODEX_HOME || join(home, ".codex");
    if (!snapshot(join(codexHome, "sessions"), dirs.codex)) {
      mkdirSync(dirs.codex, { recursive: true });
    }
    if (!snapshot(join(home, ".pi/agent/sessions"), dirs.pi)) {
      mkdirSync(dirs.pi, { recursive: true });
    }
    snapshotOpencode(opencodeDataDir(home), dirs.opencode);
    const env = {
      PATH: process.env.PATH ?? "",
      HOME: home,
      PEEKTRACE_NO_TELEMETRY: "1",
      PEEKTRACE_CLIENT_DIR: dist,
      PEEKTRACE_CLAUDE_PROJECTS: dirs.projects,
      PEEKTRACE_CODEX_SESSIONS: dirs.codex,
      PEEKTRACE_PI_SESSIONS: dirs.pi,
      PEEKTRACE_OPENCODE_DATA: dirs.opencode,
    };
    await withPair("real-features", env, ["--read-only"], async (pair) => {
      await memoryReadCases(c, pair, "real", []);
      for (const q of REAL_QUERIES) {
        await searchCase(c, pair, "real", { q });
        await searchCase(c, pair, "real", { q, limit: 0 });
      }
      if (realStats) {
        await realStatsCases(c, pair, await newestSessions(pair));
      } else {
        console.log("serve real stats: skipped (pass --real-stats to run)");
      }
    });
    const homes = { ts: join(root, "mem-ts"), rust: join(root, "mem-rust") };
    const slug = realMemoryHomes(dirs.projects, Object.values(homes));
    const tmp = join(root, "tmp");
    mkdirSync(tmp, { recursive: true });
    if (slug !== undefined) {
      await withPair(
        "real-memory-writes",
        (side) => ({
          PATH: process.env.PATH ?? "",
          HOME: homes[side],
          TMPDIR: tmp,
          PEEKTRACE_NO_TELEMETRY: "1",
          PEEKTRACE_CLIENT_DIR: dist,
          BUN_RUNTIME_TRANSPILER_CACHE_PATH: "0",
        }),
        [],
        (pair) => realMemoryWrites(c, pair, homes, slug)
      );
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};
