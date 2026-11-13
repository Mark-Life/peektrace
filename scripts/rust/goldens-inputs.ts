/**
 * The tracked inputs each golden stage depends on, and their hash. A stage
 * is stale when any of its own inputs changed, so an inspector edit stales
 * `serve` (and `ui` when it touches the helpers the app ports) and a stage
 * script edit stales the stages that import it.
 */
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, normalize } from "node:path";

export const STAGES = [
  "base",
  "search",
  "memory",
  "stats",
  "serve",
  "cli",
  "tui",
  "ui",
] as const;
export type Stage = (typeof STAGES)[number];

/** The module each stage starts from; `parity.ts` is shared by all. */
const STAGE_ENTRIES: Record<Stage, readonly string[]> = {
  base: ["parity-analyze.ts", "parity-opencode.ts"],
  search: ["parity-search.ts"],
  memory: ["parity-memory.ts"],
  stats: ["parity-stats.ts"],
  serve: ["parity-serve.ts"],
  cli: ["parity-cli.ts"],
  tui: ["parity-tui.ts"],
  ui: ["parity-ui.ts"],
};
const SCRIPTS = "scripts/rust";
const SHARED_ENTRY = `${SCRIPTS}/parity.ts`;

/** TS inputs every stage reads: the CLI and the packages it imports (not
 * the inspector UI ones), the fixtures, dependencies. */
const SHARED_SOURCE =
  /^(apps\/cli\/src\/|packages\/(?!ui\/|viz\/)[^/]+\/(src\/|package\.json$)|packages\/core\/test\/fixtures\/|bun\.lock$|package\.json$)/;
/** The inspector and its UI packages: only `serve` returns them. */
const SERVE_SOURCE =
  /^(apps\/inspector\/(?!dist\/|test\/)|packages\/(ui|viz)\/(src\/|package\.json$))/;
/** The inspector helpers and chart the Rune app ports: `ui` answers with
 * them in process. */
const UI_SOURCE =
  /^(apps\/inspector\/(src\/lib\/|src\/routes\/sessions\/|package\.json$)|packages\/(ui|viz)\/(src\/|package\.json$))/;
/** Paths `git ls-files` lists for the patterns above. */
const TREE = [
  "apps/cli/src",
  "apps/inspector",
  "packages",
  "bun.lock",
  "package.json",
];

const RELATIVE_IMPORT = /(?:from|import)\s*\(?\s*["'](\.{1,2}\/[^"']+)["']/g;
const EXTENSIONS = ["", ".ts", ".tsx", "/index.ts", "/index.tsx"];
const TS_FILE = /\.tsx?$/;
const TEST_FILE = /\.test\.tsx?$/;

/** Repo-relative paths of the relative imports in `text`, a module at `file`. */
export const importsIn = (file: string, text: string) =>
  [...text.matchAll(RELATIVE_IMPORT)].map((m) =>
    normalize(join(dirname(file), m[1] ?? ""))
  );

const resolveModule = (repo: string, path: string) =>
  EXTENSIONS.map((ext) => `${path}${ext}`).find(
    (p) => TS_FILE.test(p) && existsSync(join(repo, p))
  );

/** Script modules reachable from `entries` by relative import, leaving out
 * `stop` and what only it reaches. Paths outside `scripts/rust` are kept, not walked:
 * the tree patterns cover them. */
export const scriptClosure = (
  repo: string,
  entries: readonly string[],
  stop: ReadonlySet<string> = new Set()
) => {
  const seen = new Set<string>();
  const queue = [...entries];
  while (queue.length > 0) {
    const file = queue.pop() as string;
    if (seen.has(file) || stop.has(file)) {
      continue;
    }
    seen.add(file);
    if (!file.startsWith(`${SCRIPTS}/`)) {
      continue;
    }
    const text = readFileSync(join(repo, file), "utf8");
    for (const target of importsIn(file, text)) {
      const found = resolveModule(repo, target);
      if (found !== undefined && !seen.has(found)) {
        queue.push(found);
      }
    }
  }
  return seen;
};

const gitFiles = (repo: string) =>
  Bun.spawnSync(["git", "ls-files", "-z", "--", ...TREE], { cwd: repo })
    .stdout.toString()
    .split("\0")
    .filter(Boolean);

const entriesOf = (stage: Stage) =>
  STAGE_ENTRIES[stage].map((f) => `${SCRIPTS}/${f}`);

/** Every tracked input of `stage`, sorted. */
export const stageFiles = (
  stage: Stage,
  repo: string,
  tracked: readonly string[] = gitFiles(repo)
) => {
  const scripts = [
    ...scriptClosure(repo, [SHARED_ENTRY], new Set(STAGES.flatMap(entriesOf))),
    ...scriptClosure(repo, entriesOf(stage)),
  ];
  const tree = tracked.filter(
    (f) =>
      !TEST_FILE.test(f) &&
      (SHARED_SOURCE.test(f) ||
        (stage === "serve" && SERVE_SOURCE.test(f)) ||
        (stage === "ui" && UI_SOURCE.test(f)))
  );
  return [...new Set([...tree, ...scripts])]
    .filter((f) => existsSync(join(repo, f)))
    .sort();
};

/** sha256 over the inputs of `stage`, path and bytes. */
export const tsSourceHash = (
  stage: Stage,
  repo: string,
  tracked: readonly string[] = gitFiles(repo)
) => {
  const h = createHash("sha256");
  for (const f of stageFiles(stage, repo, tracked)) {
    h.update(`${f}\0`);
    h.update(readFileSync(join(repo, f)));
    h.update("\0");
  }
  return h.digest("hex");
};

/** Hashes of every stage, sharing one `git ls-files`. */
export const stageHashes = (repo: string) => {
  const tracked = gitFiles(repo);
  return Object.fromEntries(
    STAGES.map((s) => [s, tsSourceHash(s, repo, tracked)])
  ) as Record<Stage, string>;
};
