/** Which dirs and files Claude Code reads project instructions from. */
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import { MAX_INDEX_BYTES, MAX_INDEX_LINES } from "../memory/types";

const SLASHES = /\/+/g;
const TRAILING_SLASH = /\/$/;
const GITDIR_LINE = /^gitdir:\s*(.+?)\s*$/m;
const WORKTREE_GITDIR = /^(.*)\/\.git\/worktrees\/[^/]+$/;

/** Instruction files Claude Code reads from each project dir. */
export const CLAUDE_FILES = ["CLAUDE.md", ".claude/CLAUDE.md"];
/** Read in place of `CLAUDE_FILES` when no project dir has a CLAUDE file. */
export const AGENTS_FILES = ["AGENTS.md", ".claude/AGENTS.md"];
/** Read from every dir, also where project files are skipped. */
export const LOCAL_FILE = "CLAUDE.local.md";

const normalize = (p: string) =>
  p.replace(SLASHES, "/").replace(TRAILING_SLASH, "") || "/";

/** Resolve `.` and `..` segments of an absolute path. */
const resolveDots = (p: string) => {
  const out: string[] = [];
  for (const part of p.split("/")) {
    if (part === "..") {
      out.pop();
    } else if (part !== "" && part !== ".") {
      out.push(part);
    }
  }
  return `/${out.join("/")}`;
};

const parentOf = (d: string) => d.slice(0, d.lastIndexOf("/")) || "/";

/** `p` is `root` or a path under it. */
const within = (p: string, root: string) =>
  p === root || p.startsWith(`${root}/`);

/** `cwd` and every ancestor except `/`, farthest first, as Claude Code walks. */
export const projectDirs = (cwd: string) => {
  const out: string[] = [];
  for (let d = normalize(cwd); d !== "/" && d !== ""; d = parentOf(d)) {
    out.push(d);
  }
  return out.reverse();
};

/** Worktree roots from the `.git` file at `dir`, when nested in its main checkout. */
const worktreeOf = (dir: string, gitFile: string) => {
  const raw = GITDIR_LINE.exec(gitFile)?.[1];
  if (raw === undefined) {
    return null;
  }
  const gitdir = resolveDots(raw.startsWith("/") ? raw : `${dir}/${raw}`);
  const main = WORKTREE_GITDIR.exec(gitdir)?.[1];
  return main !== undefined && main !== dir && within(dir, main)
    ? { worktreeRoot: dir, mainRepoRoot: main }
    : null;
};

type WorktreeRoots = NonNullable<ReturnType<typeof worktreeOf>>;

/**
 * Roots of a git worktree nested inside its main checkout, or null. Claude
 * Code skips project files (not `CLAUDE.local.md`) in dirs of the main
 * checkout that sit outside the worktree, so its copy is not read twice.
 */
export const nestedWorktree = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly dirs: readonly string[];
}): Effect.Effect<WorktreeRoots | null> =>
  Effect.gen(function* () {
    const { fs } = args;
    for (const dir of [...args.dirs].reverse()) {
      const dotGit = `${dir}/.git`;
      const info = yield* fs
        .stat(dotGit)
        .pipe(Effect.orElseSucceed(() => null));
      if (info?.type === "Directory") {
        return null;
      }
      if (info?.type === "File") {
        const text = yield* fs
          .readFileString(dotGit)
          .pipe(Effect.orElseSucceed(() => ""));
        return worktreeOf(dir, text);
      }
    }
    return null;
  });

/** A dir of the main checkout outside the nested worktree. */
export const outsideWorktree = (dir: string, tree: WorktreeRoots | null) =>
  tree !== null &&
  within(dir, tree.mainRepoRoot) &&
  !within(dir, tree.worktreeRoot);

/** Claude Code caps the MEMORY.md it injects at 200 lines, then 25,000 bytes. */
export const capMemory = (text: string) => {
  const lines = text.split("\n");
  const head =
    lines.length > MAX_INDEX_LINES
      ? lines.slice(0, MAX_INDEX_LINES).join("\n")
      : text;
  const bytes = new TextEncoder().encode(head);
  return bytes.length > MAX_INDEX_BYTES
    ? new TextDecoder().decode(bytes.slice(0, MAX_INDEX_BYTES))
    : head;
};
