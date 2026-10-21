/**
 * The memory files a rule would live in, read once per report.
 *
 * What each agent loads, and only for the calls of that agent: the global
 * `CLAUDE.md` and `rules/**.md` of every Claude source, for that source's
 * sessions; Codex's `AGENTS.override.md`, else its
 * `AGENTS.md`; Pi's first context file in `~/.pi/agent`; OpenCode's
 * `~/.config/opencode/AGENTS.md`, else its `~/.claude/CLAUDE.md` fallback. A
 * moved `XDG_CONFIG_HOME`, and OpenCode's env switch that turns the fallback
 * off, are not followed: the report never reads the process env.
 *
 * Per project, for each directory the cause table names, each agent's own
 * files: Claude's `CLAUDE.md`, `CLAUDE.local.md`, `.claude/CLAUDE.md` and
 * every `.md` under `.claude/rules/` (a rule scoped by `paths:` frontmatter is
 * read as if it always loaded); the
 * first of Codex's, Pi's or OpenCode's names that exists. Claude, Pi and
 * OpenCode read every ancestor up to the user's home, or up to the root for a
 * dir outside it. Codex stops at the nearest ancestor holding `.git` (its
 * default `project_root_markers`) and reads only the directory itself when
 * none has one, so a Codex project file reaches every working directory under
 * it in the same repo, but not one inside a nested repo the sampled dirs
 * found, nor any but itself outside a repo. A nested repo no sampled dir ran
 * in is not seen. A project
 * `AGENTS.md` is never credited to Claude, nor a `CLAUDE.md` to Codex. A
 * file's `@path` imports are followed, up to Claude Code's depth limit, and
 * read as part of the file that imports them: same scope, same reach.
 *
 * Everything goes through the platform FileSystem and the agent registry's
 * roots — never the process's own home — so a test stub stays isolated and a
 * remote report reads the server host's files. Symlinks are followed and
 * files are deduped by where they really live and what they reach: a global
 * `CLAUDE.md` that links to `~/.agents/AGENTS.md` is one file for Claude, and
 * the Codex `AGENTS.md` that links there too is the same file for Codex.
 * Paths are handled with `/`; a Windows `\` is read as one.
 */
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import type { AgentId } from "../agent-id";
import { type AgentRegistryShape, sourcesOf } from "../agents";
import { readContextText } from "../sessions/resolve";
import type { MemoryDoc, RuleCover } from "./detectors/rules";

export type { MemoryDoc } from "./detectors/rules";

const SLASHES = /[\\/]+/g;
/** A per-user agent dir under the user's home, for the home it sits in. */
const AGENT_HOME = /^(.+)\/\.(?:claude|codex|pi)\/?$/;
/** `/abs`, or a Windows drive path `C:/abs`. */
const ABSOLUTE = /^(?:\/|[A-Za-z]:\/)/;
const NEWLINE = /\r?\n/;

/**
 * The files an agent reads from one directory: every name, or only the first
 * that exists.
 */
interface FileSet {
  readonly names: readonly string[];
  readonly pick: "all" | "first";
}

/** Codex's names, global and per project: the override shadows the file. */
const CODEX_FILES: FileSet = {
  pick: "first",
  names: ["AGENTS.override.md", "AGENTS.md"],
};

/** Pi's `loadContextFileFromDir`, for its agent dir and each project dir. */
const PI_FILES: FileSet = {
  pick: "first",
  names: [
    "AGENTS.override.md",
    "AGENTS.md",
    "AGENTS.MD",
    "CLAUDE.md",
    "CLAUDE.MD",
  ],
};

/** Files each agent reads from a project directory or any of its ancestors. */
const PROJECT_FILES: Readonly<Record<AgentId, FileSet>> = {
  claude: {
    pick: "all",
    names: ["CLAUDE.md", "CLAUDE.local.md", ".claude/CLAUDE.md"],
  },
  codex: CODEX_FILES,
  pi: PI_FILES,
  opencode: { pick: "first", names: ["AGENTS.md", "CLAUDE.md"] },
};

/** Import hops Claude Code follows from a memory file. */
const IMPORT_DEPTH = 5;

/** `@path` at a word start: `@~/x.md`, `@./docs/a.md`, `@/abs/b.md`. */
const IMPORT = /(?:^|\s)@((?:~\/|\.{1,2}\/|\/)?[\w.~/-]+)/g;
/** An import names a file: a bare `@name` is a mention, not a path. */
const PATHLIKE = /[/.]/;
const FENCE = /^\s*(?:```|~~~)/;
const CODE_SPAN = /`[^`]*`/g;
const TRAILING_PUNCT = /[.,;:)]+$/;
const TRAILING_SLASH = /\/$/;

/** A path with every separator as one `/`. */
const slashed = (path: string): string => path.replace(SLASHES, "/");

const join = (...parts: readonly string[]): string => slashed(parts.join("/"));

const dirname = (path: string): string => {
  const at = path.lastIndexOf("/");
  return at <= 0 ? "/" : path.slice(0, at);
};

/**
 * `dir` and its ancestors, nearest first: up to the user's home for a dir
 * under it (Claude Code walks on to `/`, but nothing above the home is the
 * user's to edit), else up to the filesystem root, as every agent walks. A
 * repo outside the home (`/workspace`, `/srv`, `D:/`) keeps its root file.
 */
const ancestors = (dir: string, userHome: string | null): readonly string[] => {
  const start = slashed(dir).replace(TRAILING_SLASH, "");
  if (start === "") {
    return slashed(dir) === "/" ? ["/"] : [];
  }
  const stop =
    userHome !== null && start.startsWith(`${userHome}/`) ? userHome : null;
  const out: string[] = [];
  for (let d = start; ; ) {
    out.push(d);
    const at = d.lastIndexOf("/");
    if (d === stop || d === "/" || at < 0) {
      return out;
    }
    d = at === 0 ? "/" : d.slice(0, at);
  }
};

/** Collapse the user's home to `~`, given the home a Claude root sits in. */
const tilde = (path: string, userHome: string | null): string =>
  userHome !== null && (path === userHome || path.startsWith(`${userHome}/`))
    ? `~${path.slice(userHome.length)}`
    : path;

/** How Codex's walk reaches the cwds under one project dir. */
type CodexWalk = NonNullable<RuleCover["codex"]>;

/**
 * A file to read, or a Claude rules dir whose `.md` files are read, with the
 * reach it inherits.
 */
interface Candidate {
  readonly cover: RuleCover;
  readonly depth: number;
  /**
   * Candidates of one group stop at the first file that exists; a file read
   * for its own sake is a group of one.
   */
  readonly group: string;
  readonly kind: "file" | "rules";
  readonly path: string;
}

/** Where a file applies, so one file read for two agents is kept for both. */
const coverKey = (cover: RuleCover): string =>
  [
    cover.agent,
    cover.dir ?? "",
    cover.source?.id ?? "",
    cover.codex === undefined
      ? ""
      : `${cover.codex.exact}\0${cover.codex.stops.join("\0")}`,
  ].join("\0");

/** The candidates of one file set in one directory, for one reach. */
const fileSetIn = (args: {
  readonly dir: string;
  readonly files: FileSet;
  readonly cover: RuleCover;
}): readonly Candidate[] => {
  const { dir, files, cover } = args;
  const first = `${dir}\0${coverKey(cover)}`;
  return files.names.map((name) => {
    const path = join(dir, name);
    return {
      path,
      cover,
      group: files.pick === "first" ? first : `${path}\0${coverKey(cover)}`,
      depth: 0,
      kind: "file" as const,
    };
  });
};

/** A Claude rules dir: every `.md` under it, recursively. */
const rulesDirIn = (dir: string, cover: RuleCover): Candidate => {
  const path = join(dir, "rules");
  return {
    path,
    cover,
    group: `${path}\0${coverKey(cover)}`,
    depth: 0,
    kind: "rules",
  };
};

/** Each in-scope agent's global files, read for every call of that agent. */
const globalCandidates = (args: {
  readonly agents: AgentRegistryShape;
  readonly scope: readonly AgentId[];
  readonly userHome: string | null;
}): readonly Candidate[] => {
  const { agents, scope, userHome } = args;
  const out: Candidate[] = [];
  if (scope.includes("claude")) {
    const sources = sourcesOf(agents.roots("claude"));
    for (const source of sources) {
      const cover: RuleCover =
        sources.length > 1
          ? {
              agent: "claude",
              dir: null,
              source: { id: source.id, isDefault: source.isDefault },
            }
          : { agent: "claude", dir: null };
      out.push(
        ...fileSetIn({
          dir: source.home,
          files: { pick: "all", names: ["CLAUDE.md"] },
          cover,
        }),
        rulesDirIn(source.home, cover)
      );
    }
  }
  if (scope.includes("codex")) {
    out.push(
      ...fileSetIn({
        dir: agents.roots("codex").home,
        files: CODEX_FILES,
        cover: { agent: "codex", dir: null },
      })
    );
  }
  if (scope.includes("pi")) {
    out.push(
      ...fileSetIn({
        dir: join(agents.roots("pi").home, "agent"),
        files: PI_FILES,
        cover: { agent: "pi", dir: null },
      })
    );
  }
  if (scope.includes("opencode") && userHome !== null) {
    out.push(
      ...fileSetIn({
        dir: userHome,
        files: {
          pick: "first",
          names: [".config/opencode/AGENTS.md", ".claude/CLAUDE.md"],
        },
        cover: { agent: "opencode", dir: null },
      })
    );
  }
  return out;
};

/** Every file worth reading, in priority order: the first of a realPath wins. */
const candidatesOf = (args: {
  readonly agents: AgentRegistryShape;
  readonly scope: readonly AgentId[];
  readonly projectDirs: readonly string[];
  readonly userHome: string | null;
  /** For each Codex project dir, the walk that reads it. */
  readonly codexDirs: ReadonlyMap<string, CodexWalk>;
}): readonly Candidate[] => {
  const { scope, projectDirs, userHome, codexDirs } = args;
  const out: Candidate[] = [...globalCandidates(args)];
  const dirs = [
    ...new Set(projectDirs.flatMap((dir) => ancestors(dir, userHome))),
  ].sort();
  for (const dir of dirs) {
    for (const agent of scope) {
      const codex = agent === "codex" ? codexDirs.get(dir) : undefined;
      if (agent === "codex" && codex === undefined) {
        continue;
      }
      const files = PROJECT_FILES[agent];
      out.push(
        ...fileSetIn({
          dir,
          // `~/.claude/CLAUDE.md` is the global file, never a project one.
          files:
            dir === userHome
              ? {
                  ...files,
                  names: files.names.filter((n) => n !== ".claude/CLAUDE.md"),
                }
              : files,
          cover: codex === undefined ? { agent, dir } : { agent, dir, codex },
        })
      );
      // `~/.claude/rules` is the global rules dir, read above per source.
      if (agent === "claude" && dir !== userHome) {
        out.push(rulesDirIn(join(dir, ".claude"), { agent, dir }));
      }
    }
  }
  return out;
};

/** True when `path` is `dir` or sits under it. */
const isUnder = (path: string, dir: string): boolean =>
  path === dir || path.startsWith(dir === "/" ? "/" : `${dir}/`);

/**
 * For each directory Codex reads project files from, the walk that reads it.
 * Codex walks up from the cwd to the nearest ancestor holding a `.git` (a dir,
 * or a worktree's file) and reads that root down to the cwd; with no root it
 * reads the cwd alone. The walk is bounded like the other agents': a root
 * above the home is not looked for, one outside it is.
 */
const codexDirsOf = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly projectDirs: readonly string[];
  readonly userHome: string | null;
}): Effect.Effect<ReadonlyMap<string, CodexWalk>> =>
  Effect.gen(function* () {
    const { fs, projectDirs, userHome } = args;
    /** Each read dir, and whether some walk that reads it found a root. */
    const read = new Map<string, boolean>();
    const roots = new Set<string>();
    for (const cwd of projectDirs) {
      const chain = ancestors(cwd, userHome);
      let walk = chain.slice(0, 1);
      let rooted = false;
      for (const [i, dir] of chain.entries()) {
        const marked = yield* fs
          .exists(join(dir, ".git"))
          .pipe(Effect.orElseSucceed(() => false));
        if (marked) {
          walk = chain.slice(0, i + 1);
          rooted = true;
          roots.add(dir);
          break;
        }
      }
      for (const dir of walk) {
        read.set(dir, (read.get(dir) ?? false) || rooted);
      }
    }
    const out = new Map<string, CodexWalk>();
    for (const [dir, rooted] of read) {
      out.set(dir, {
        exact: !rooted,
        stops: rooted
          ? [...roots].filter((r) => r !== dir && isUnder(r, dir)).sort()
          : [],
      });
    }
    return out;
  });

/** Resolve one import: `~/` against the user's home, else the importer's dir. */
const resolveImport = (args: {
  readonly raw: string;
  readonly from: string;
  readonly userHome: string | null;
}): string | null => {
  const { raw, from, userHome } = args;
  if (raw.startsWith("~/")) {
    return userHome === null ? null : join(userHome, raw.slice(2));
  }
  return ABSOLUTE.test(raw) ? raw : join(dirname(from), raw);
};

/** Lines outside code fences, with code spans blanked: examples, not imports. */
const proseOf = (lines: readonly string[]): readonly string[] => {
  const out: string[] = [];
  let fenced = false;
  for (const line of lines) {
    if (FENCE.test(line)) {
      fenced = !fenced;
    } else if (!fenced) {
      out.push(line.replace(CODE_SPAN, ""));
    }
  }
  return out;
};

/** The files a memory file imports with `@path`, resolved. */
const importsOf = (args: {
  readonly lines: readonly string[];
  readonly from: string;
  readonly userHome: string | null;
}): readonly string[] =>
  proseOf(args.lines).flatMap((line) =>
    [...line.matchAll(IMPORT)].flatMap((m) => {
      const raw = (m[1] ?? "").replace(TRAILING_PUNCT, "");
      const path = PATHLIKE.test(raw) ? resolveImport({ ...args, raw }) : null;
      return path === null ? [] : [path];
    })
  );

/**
 * The user's home, from the first agent dir that sits right under it. Claude's
 * may be moved by `CLAUDE_CONFIG_DIR` (`~/.claude-work`), Codex's by
 * `CODEX_HOME`; Pi's always lives at `~/.pi`.
 */
const userHomeOf = (agents: AgentRegistryShape): string | null => {
  for (const id of ["claude", "codex", "pi"] as const) {
    const home = AGENT_HOME.exec(slashed(agents.roots(id).home))?.[1];
    if (home !== undefined) {
      return home;
    }
  }
  return null;
};

/** The files one read file imports, as candidates with its reach. */
const importCandidates = (args: {
  readonly candidate: Candidate;
  readonly lines: readonly string[];
  readonly from: string;
  readonly userHome: string | null;
}): readonly Candidate[] => {
  const { candidate } = args;
  if (candidate.depth >= IMPORT_DEPTH) {
    return [];
  }
  return importsOf(args).map((path) => ({
    ...candidate,
    path,
    group: `${path}\0${coverKey(candidate.cover)}`,
    depth: candidate.depth + 1,
    kind: "file",
  }));
};

const MARKDOWN = /\.md$/i;

/** The `.md` files under a rules dir, as file candidates. None when absent. */
const rulesOf = (
  fs: FileSystem.FileSystem,
  dir: Candidate
): Effect.Effect<readonly Candidate[]> =>
  fs.readDirectory(dir.path, { recursive: true }).pipe(
    Effect.map((names) =>
      names
        .filter((name) => MARKDOWN.test(name))
        .map(slashed)
        .sort()
        .map((name): Candidate => {
          const path = join(dir.path, name);
          return {
            ...dir,
            path,
            group: `${path}\0${coverKey(dir.cover)}`,
            kind: "file",
          };
        })
    ),
    Effect.orElseSucceed((): readonly Candidate[] => [])
  );

/** Read every memory file in scope. A missing file is simply not there. */
export const readMemoryDocs = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly scope: readonly AgentId[];
  readonly projectDirs: readonly string[];
}): Effect.Effect<readonly MemoryDoc[]> => {
  const { fs, agents } = args;
  const userHome = userHomeOf(agents);
  return Effect.gen(function* () {
    const docs: MemoryDoc[] = [];
    const seen = new Set<string>();
    /** Groups whose first existing file was already read. */
    const settled = new Set<string>();
    const codexDirs = args.scope.includes("codex")
      ? yield* codexDirsOf({ fs, projectDirs: args.projectDirs, userHome })
      : new Map<string, CodexWalk>();
    const queue = [...candidatesOf({ ...args, userHome, codexDirs })];
    for (let next = queue.shift(); next !== undefined; next = queue.shift()) {
      const candidate = next;
      if (settled.has(candidate.group)) {
        continue;
      }
      if (candidate.kind === "rules") {
        // Read in name order, right here, ahead of the queue.
        queue.unshift(...(yield* rulesOf(fs, candidate)));
        continue;
      }
      const read = yield* readContextText({ fs, path: candidate.path });
      if (read === null) {
        continue;
      }
      settled.add(candidate.group);
      const realPath = slashed(read.realPath);
      const key = `${realPath}\0${coverKey(candidate.cover)}`;
      if (seen.has(key)) {
        continue;
      }
      seen.add(key);
      const lines = read.text.split(NEWLINE);
      const shown = tilde(candidate.path, userHome);
      docs.push({
        displayPath:
          realPath === candidate.path
            ? shown
            : `${shown} → ${tilde(realPath, userHome)}`,
        realPath,
        cover: candidate.cover,
        mtimeMs: read.mtimeMs,
        lines,
      });
      // Imports are read right after their file, ahead of the queue, so an
      // imported rule is credited to the file that pulled it in.
      queue.unshift(
        ...importCandidates({ candidate, lines, from: realPath, userHome })
      );
    }
    return docs;
  }).pipe(Effect.withSpan("Stats.memoryDocs"));
};
