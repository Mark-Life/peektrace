/** Which dirs and files Claude Code reads project instructions from. */
import { posix } from "node:path";
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import { MAX_INDEX_LINES } from "../memory/types";
import { rulePaths } from "./context-rules";

const { dirname, join, resolve } = posix;

const GITDIR_LINE = /^gitdir:\s*(.+?)\s*$/m;
const WORKTREE_GITDIR = /^(.*)\/\.git\/worktrees\/[^/]+$/;
/** Claude Code cuts MEMORY.md past this many characters, at a newline. */
const MEMORY_MAX_CHARS = 25_000;

/** Instruction files Claude Code reads from each project dir. */
export const CLAUDE_FILES = ["CLAUDE.md", ".claude/CLAUDE.md"];
/** Read in place of `CLAUDE_FILES` when no project dir has a CLAUDE file. */
export const AGENTS_FILES = ["AGENTS.md", ".claude/AGENTS.md"];
/** Read from every dir, also where project files are skipped. */
export const LOCAL_FILE = "CLAUDE.local.md";
const RULES_DIR = ".claude/rules";

/** `p` is `root` or a path under it. */
const within = (p: string, root: string) =>
  p === root || p.startsWith(`${root}/`);

/** `cwd` and every ancestor except `/`, farthest first, as Claude Code walks. */
export const projectDirs = (cwd: string) => {
  const out: string[] = [];
  for (let d = resolve("/", cwd); d !== "/"; d = dirname(d)) {
    out.push(d);
  }
  return out.reverse();
};

/** The git checkout holding `cwd`: its root and its main checkout's root. */
interface Checkout {
  readonly mainRoot: string;
  readonly root: string;
}

/** Main checkout root named by a worktree's `.git` file, else `dir`. */
const mainRootOf = (dir: string, gitFile: string) => {
  const raw = GITDIR_LINE.exec(gitFile)?.[1];
  return raw === undefined
    ? dir
    : (WORKTREE_GITDIR.exec(resolve(dir, raw))?.[1] ?? dir);
};

/** The nearest checkout among `dirs` (farthest first), or null outside git. */
export const gitCheckout = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly dirs: readonly string[];
}): Effect.Effect<Checkout | null> =>
  Effect.gen(function* () {
    const { fs } = args;
    for (const dir of [...args.dirs].reverse()) {
      const dotGit = join(dir, ".git");
      const info = yield* fs
        .stat(dotGit)
        .pipe(Effect.orElseSucceed(() => null));
      if (info?.type === "Directory") {
        return { root: dir, mainRoot: dir };
      }
      if (info?.type === "File") {
        const text = yield* fs
          .readFileString(dotGit)
          .pipe(Effect.orElseSucceed(() => ""));
        return { root: dir, mainRoot: mainRootOf(dir, text) };
      }
    }
    return null;
  });

/**
 * A dir of the main checkout outside a worktree nested in it. Claude Code
 * skips project files (not `CLAUDE.local.md`) there, so the main checkout's
 * copy is not read twice.
 */
const outsideWorktree = (dir: string, co: Checkout | null) =>
  co !== null &&
  co.mainRoot !== co.root &&
  within(dir, co.mainRoot) &&
  !within(dir, co.root);

/**
 * Project instruction files Claude Code loads for `cwd`, in its order, and
 * the root its auto-memory dir is keyed by (the main git checkout, else
 * `cwd`). `home` is the Claude config dir, whose files are user-level.
 */
export const projectContext = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly cwd: string;
  readonly home: string;
}) =>
  Effect.gen(function* () {
    const { fs, cwd, home } = args;
    const dirs = projectDirs(cwd);
    const co = yield* gitCheckout({ fs, dirs });
    const userLevel = new Set([join(home, "CLAUDE.md"), join(home, "rules")]);
    const projectFiles = (dir: string, names: readonly string[]) =>
      outsideWorktree(dir, co) ? [] : names.map((n) => join(dir, n));
    // Claude Code's default `claude-md-or-agents-md`: a project with no
    // CLAUDE file of its own reads AGENTS.md where CLAUDE.md would be.
    const isClaudeProject = yield* Effect.reduce(
      dirs.flatMap((d) => [
        ...projectFiles(d, CLAUDE_FILES),
        join(d, LOCAL_FILE),
      ]),
      false,
      (found, path) =>
        found || userLevel.has(path)
          ? Effect.succeed(found)
          : fs.exists(path).pipe(Effect.orElseSucceed(() => false))
    );
    const names = isClaudeProject ? CLAUDE_FILES : AGENTS_FILES;
    const paths: string[] = [];
    for (const dir of dirs) {
      const rulesDir = join(dir, RULES_DIR);
      const rules =
        outsideWorktree(dir, co) || userLevel.has(rulesDir)
          ? []
          : yield* rulePaths({ fs, dir: rulesDir });
      paths.push(...projectFiles(dir, names), ...rules, join(dir, LOCAL_FILE));
    }
    return {
      paths: paths.filter((p) => !userLevel.has(p)),
      memoryRoot: co?.mainRoot ?? cwd,
    };
  });

/** Claude Code's project dir slug: every UTF-16 unit outside `[a-zA-Z0-9]` becomes `-`. */
export const claudeSlug = (path: string) => path.replace(/[^a-zA-Z0-9]/g, "-");

/**
 * Claude Code trims MEMORY.md, keeps 200 lines, then cuts past 25,000
 * characters at the last newline before the limit.
 */
export const capMemory = (text: string) => {
  const trimmed = text.trim();
  const lines = trimmed.split("\n");
  const head =
    lines.length > MAX_INDEX_LINES
      ? lines.slice(0, MAX_INDEX_LINES).join("\n")
      : trimmed;
  if (head.length <= MEMORY_MAX_CHARS) {
    return head;
  }
  const cut = head.lastIndexOf("\n", MEMORY_MAX_CHARS);
  return head.slice(0, cut > 0 ? cut : MEMORY_MAX_CHARS);
};
