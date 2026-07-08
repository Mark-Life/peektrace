import { execFileSync } from "node:child_process";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { FileSystem } from "@effect/platform";
import { Context, Data, Effect, Layer } from "effect";
import { AGENT_IDS, type AgentId } from "./agent-id";

export { AGENT_IDS, AgentId } from "./agent-id";

/** How an agent lays its transcripts out on disk. Drives `listSessionFiles`. */
export type SessionLayout =
  /** `~/.claude/projects/<slug>/<id>.jsonl` (+ `<id>/subagents/…`). */
  | "claude-projects"
  /** `~/.codex/sessions/YYYY/MM/DD/rollout-<iso>-<uuid>.jsonl` (date tree). */
  | "codex-datetree"
  /** `~/.pi/agent/sessions/<cwd-slug>/<iso>_<uuid>.jsonl`. */
  | "pi-cwd-slug"
  /** No parseable session layout yet. */
  | "none";

/** On-disk roots declared for an agent. */
export interface AgentRoots {
  /** Base config dir, e.g. ~/.claude. */
  readonly home: string;
  readonly id: AgentId;
  /** On-disk transcript layout; selects the `listSessionFiles` walker. */
  readonly layout: SessionLayout;
  /** Session-transcript root, e.g. ~/.claude/projects or ~/.codex/sessions. */
  readonly projectsRoot: string;
  /** True for agents whose transcripts can be listed + parsed (claude/codex/pi). */
  readonly supported: boolean;
}

/** One transcript located on disk, before its body is parsed. */
export interface SessionFileRef {
  /** Stable session id (filename stem for Claude/Pi, rollout uuid for Codex). */
  readonly id: string;
  /** Absolute path to the `.jsonl` transcript. */
  readonly path: string;
  /** Owning project slug, or `""` when the layout has none (Codex date tree). */
  readonly slug: string;
}

/** Raised when a resolver is invoked for an agent that is declared but unimplemented. */
export class AgentUnsupportedError extends Data.TaggedError(
  "AgentUnsupportedError"
)<{
  readonly agent: AgentId;
  readonly operation: string;
}> {}

/**
 * Build the declared roots for every agent. Computed lazily (and memoized) so
 * importing this module never touches Node's `os`/`path` or `process.env` — the
 * RPC contract pulls these schemas into the browser bundle, where those builtins
 * are stubbed and would throw at import time if invoked eagerly.
 *
 * Claude's projects root defaults to `~/.claude/projects` but may be overridden
 * via `PEEKTRACE_CLAUDE_PROJECTS` so tooling (and especially automated browser
 * tests) can point at a throwaway temp dir instead of the user's real memories.
 */
const buildRoots = (): Record<AgentId, AgentRoots> => {
  const HOME = homedir();
  return {
    claude: {
      id: "claude",
      home: join(HOME, ".claude"),
      layout: "claude-projects",
      projectsRoot:
        process.env.PEEKTRACE_CLAUDE_PROJECTS ??
        join(HOME, ".claude", "projects"),
      supported: true,
    },
    codex: {
      id: "codex",
      home: join(HOME, ".codex"),
      layout: "codex-datetree",
      projectsRoot:
        process.env.PEEKTRACE_CODEX_SESSIONS ??
        join(HOME, ".codex", "sessions"),
      supported: true,
    },
    pi: {
      id: "pi",
      home: join(HOME, ".pi"),
      layout: "pi-cwd-slug",
      projectsRoot:
        process.env.PEEKTRACE_PI_SESSIONS ??
        join(HOME, ".pi", "agent", "sessions"),
      supported: true,
    },
    opencode: {
      id: "opencode",
      home: join(HOME, ".local", "share", "opencode"),
      layout: "none",
      projectsRoot: join(HOME, ".local", "share", "opencode", "project"),
      supported: false,
    },
  };
};

let rootsCache: Record<AgentId, AgentRoots> | null = null;

/** Memoized accessor for the per-agent declared roots. */
const ROOTS = (): Record<AgentId, AgentRoots> => {
  rootsCache ??= buildRoots();
  return rootsCache;
};

/** Documented Claude encoding: forward slashes AND periods both become dashes. */
const encodeSlug = (cwdPath: string): string => cwdPath.replace(/[/.]/g, "-");

const JSONL = /\.jsonl$/;
const CODEX_ROLLOUT = /^rollout-.*\.jsonl$/;
/** Any RFC-4122 uuid, as embedded in Codex rollout / Pi transcript filenames. */
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i;

/** Codex rollout id = the trailing uuid of `rollout-<iso>-<uuid>.jsonl`. */
const codexId = (filename: string): string =>
  UUID.exec(filename)?.[0] ?? filename.replace(JSONL, "");

/** Pi session id = the uuid after `_` in `<iso>_<uuid>.jsonl`. */
const piId = (filename: string): string => {
  const stem = filename.replace(JSONL, "");
  const afterUnderscore = stem.slice(stem.indexOf("_") + 1);
  return UUID.exec(afterUnderscore)?.[0] ?? (afterUnderscore || stem);
};

/** Immediate subdirectory names of `root` (its project slugs). Never fails. */
const listSlugDirs = (
  fs: FileSystem.FileSystem,
  root: string
): Effect.Effect<readonly string[]> =>
  fs.exists(root).pipe(
    Effect.flatMap((present) =>
      present ? fs.readDirectory(root) : Effect.succeed<readonly string[]>([])
    ),
    Effect.flatMap((names) =>
      Effect.forEach(names, (name) =>
        fs.stat(join(root, name)).pipe(
          Effect.map((info) => (info.type === "Directory" ? name : null)),
          Effect.orElseSucceed(() => null)
        )
      )
    ),
    Effect.map((names) =>
      names.filter((name): name is string => name !== null)
    ),
    Effect.orElseSucceed(() => [] as readonly string[])
  );

/** Service contract for per-agent path resolution. */
export interface AgentRegistryShape {
  /** All declared root paths across agents (used by the FS wrapper for containment). */
  readonly allowedRoots: readonly string[];
  /** Encode a cwd/git-root path into a Claude project slug. */
  readonly encodeSlug: (cwdPath: string) => string;
  /** Resolve the git repo root for a dir, falling back to the dir itself. Never fails. */
  readonly gitRoot: (cwd: string) => Effect.Effect<string>;
  /** Enumerate every project slug for an agent (Claude-layout only). */
  readonly listProjectSlugs: (
    agent: AgentId
  ) => Effect.Effect<readonly string[], AgentUnsupportedError, never>;
  /**
   * Enumerate every transcript for an agent, abstracting over the three
   * on-disk layouts (Claude project slugs, Codex date tree, Pi cwd slugs).
   * Never fails: an unsupported agent (or a missing root) yields `[]`.
   */
  readonly listSessionFiles: (
    agent: AgentId
  ) => Effect.Effect<readonly SessionFileRef[]>;
  /** Per-project memory dir for an agent + slug. */
  readonly memoryDir: (args: {
    readonly agent: AgentId;
    readonly slug: string;
  }) => Effect.Effect<string, AgentUnsupportedError>;
  /** The projects root for an agent. */
  readonly projectsRoot: (
    agent: AgentId
  ) => Effect.Effect<string, AgentUnsupportedError>;
  /** Declared roots for an agent (succeeds for all; resolvers gate on `supported`). */
  readonly roots: (agent: AgentId) => AgentRoots;
  /** Glob matching every session transcript for an agent. */
  readonly sessionsGlob: (
    agent: AgentId
  ) => Effect.Effect<string, AgentUnsupportedError>;
}

/** Per-agent on-disk roots and path resolvers. Only Claude is implemented. */
export class AgentRegistry extends Context.Tag("@peektrace/AgentRegistry")<
  AgentRegistry,
  AgentRegistryShape
>() {}

/** Fail with `AgentUnsupportedError` for any agent whose resolvers are stubbed. */
const requireSupported = (
  agent: AgentId,
  operation: string
): Effect.Effect<void, AgentUnsupportedError> =>
  ROOTS()[agent].supported
    ? Effect.void
    : Effect.fail(new AgentUnsupportedError({ agent, operation }));

/**
 * Gate the project-slug / memory resolvers on Claude's `claude-projects`
 * layout. Codex (date tree) and Pi (cwd slugs) are "supported" for session
 * listing but have no per-project memory dirs, so these operations must still
 * fail for them rather than silently reading the wrong shape.
 */
const requireClaudeLayout = (
  agent: AgentId,
  operation: string
): Effect.Effect<void, AgentUnsupportedError> =>
  ROOTS()[agent].layout === "claude-projects"
    ? Effect.void
    : Effect.fail(new AgentUnsupportedError({ agent, operation }));

/** Live layer: resolves Claude paths via the platform FileSystem. */
export const AgentRegistryLive = Layer.effect(
  AgentRegistry,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;

    const allowedRoots = AGENT_IDS.flatMap((id) => [
      ROOTS()[id].home,
      ROOTS()[id].projectsRoot,
    ]);

    const gitRoot = (cwd: string) =>
      Effect.try({
        try: () =>
          execFileSync("git", ["rev-parse", "--show-toplevel"], {
            cwd,
            stdio: ["ignore", "pipe", "ignore"],
            encoding: "utf8",
          }).trim(),
        catch: () => cwd,
      }).pipe(
        Effect.map((out) => out || cwd),
        Effect.orElseSucceed(() => cwd),
        Effect.withSpan("AgentRegistry.gitRoot", { attributes: { cwd } })
      );

    const projectsRoot = (agent: AgentId) =>
      requireSupported(agent, "projectsRoot").pipe(
        Effect.as(ROOTS()[agent].projectsRoot)
      );

    const sessionsGlob = (agent: AgentId) =>
      requireSupported(agent, "sessionsGlob").pipe(
        Effect.as(join(ROOTS()[agent].projectsRoot, "**", "*.jsonl"))
      );

    const memoryDir = ({
      agent,
      slug,
    }: {
      readonly agent: AgentId;
      readonly slug: string;
    }) =>
      requireClaudeLayout(agent, "memoryDir").pipe(
        Effect.as(join(ROOTS()[agent].projectsRoot, slug, "memory"))
      );

    const listProjectSlugs = (agent: AgentId) =>
      requireClaudeLayout(agent, "listProjectSlugs").pipe(
        Effect.flatMap(() => listSlugDirs(fs, ROOTS()[agent].projectsRoot)),
        Effect.withSpan("AgentRegistry.listProjectSlugs", {
          attributes: { agent },
        })
      );

    /** Absolute paths of files directly in `dir` matching `re` (non-recursive). */
    const filesIn = (dir: string, re: RegExp) =>
      fs.readDirectory(dir).pipe(
        Effect.orElseSucceed(() => [] as string[]),
        Effect.flatMap((names) =>
          Effect.forEach(
            names.filter((n) => re.test(n)),
            (name) => {
              const path = join(dir, name);
              return fs.stat(path).pipe(
                Effect.map((info) =>
                  info.type === "File" ? { path, name } : null
                ),
                Effect.orElseSucceed(() => null)
              );
            }
          )
        ),
        Effect.map((entries) =>
          entries.filter((e): e is { path: string; name: string } => e !== null)
        )
      );

    /** Recursively collect `rollout-*.jsonl` paths under a Codex date tree. */
    const walkCodex = (dir: string): Effect.Effect<string[]> =>
      fs.readDirectory(dir).pipe(
        Effect.orElseSucceed(() => [] as string[]),
        Effect.flatMap((names) =>
          Effect.forEach(names, (name) => {
            const path = join(dir, name);
            return fs.stat(path).pipe(
              Effect.flatMap((info) =>
                info.type === "Directory"
                  ? walkCodex(path)
                  : Effect.succeed(
                      info.type === "File" && CODEX_ROLLOUT.test(name)
                        ? [path]
                        : []
                    )
              ),
              Effect.orElseSucceed(() => [] as string[])
            );
          })
        ),
        Effect.map((lists) => lists.flat())
      );

    /** Enumerate `<root>/<slug>/*.jsonl`, deriving ids via `toId`. */
    const listSlugLayout = (root: string, toId: (name: string) => string) =>
      listSlugDirs(fs, root).pipe(
        Effect.flatMap((slugs) =>
          Effect.forEach(slugs, (slug) =>
            filesIn(join(root, slug), JSONL).pipe(
              Effect.map((entries) =>
                entries.map(
                  ({ path, name }) =>
                    ({ path, slug, id: toId(name) }) satisfies SessionFileRef
                )
              )
            )
          )
        ),
        Effect.map((groups) => groups.flat())
      );

    const listSessionFiles = (agent: AgentId) => {
      const { layout, projectsRoot: root } = ROOTS()[agent];
      const build = (): Effect.Effect<readonly SessionFileRef[]> => {
        switch (layout) {
          case "claude-projects":
            return listSlugLayout(root, (name) => name.replace(JSONL, ""));
          case "pi-cwd-slug":
            return listSlugLayout(root, piId);
          case "codex-datetree":
            return walkCodex(root).pipe(
              Effect.map((paths) =>
                paths.map(
                  (path) =>
                    ({
                      path,
                      slug: "",
                      id: codexId(basename(path)),
                    }) satisfies SessionFileRef
                )
              )
            );
          default:
            return Effect.succeed([] as readonly SessionFileRef[]);
        }
      };
      return build().pipe(
        Effect.orElseSucceed(() => [] as readonly SessionFileRef[]),
        Effect.withSpan("AgentRegistry.listSessionFiles", {
          attributes: { agent },
        })
      );
    };

    return {
      encodeSlug,
      roots: (agent) => ROOTS()[agent],
      allowedRoots,
      gitRoot,
      projectsRoot,
      listProjectSlugs,
      listSessionFiles,
      sessionsGlob,
      memoryDir,
    } satisfies AgentRegistryShape;
  })
);
