import { execFileSync } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { Context, Data, Effect, Layer } from "effect";
import { AGENT_IDS, type AgentId } from "./agent-id";

export { AGENT_IDS, AgentId } from "./agent-id";

/** On-disk roots declared for an agent (only Claude is actually resolved). */
export interface AgentRoots {
  /** Base config dir, e.g. ~/.claude. */
  readonly home: string;
  readonly id: AgentId;
  /** Per-project root, e.g. ~/.claude/projects. */
  readonly projectsRoot: string;
  /** True only for agents whose resolvers are implemented (Claude). */
  readonly supported: boolean;
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
      projectsRoot:
        process.env.PEEKTRACE_CLAUDE_PROJECTS ??
        join(HOME, ".claude", "projects"),
      supported: true,
    },
    codex: {
      id: "codex",
      home: join(HOME, ".codex"),
      projectsRoot: join(HOME, ".codex", "sessions"),
      supported: false,
    },
    pi: {
      id: "pi",
      home: join(HOME, ".pi"),
      projectsRoot: join(HOME, ".pi", "projects"),
      supported: false,
    },
    opencode: {
      id: "opencode",
      home: join(HOME, ".local", "share", "opencode"),
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

/** Service contract for per-agent path resolution. */
export interface AgentRegistryShape {
  /** All declared root paths across agents (used by the FS wrapper for containment). */
  readonly allowedRoots: readonly string[];
  /** Encode a cwd/git-root path into a Claude project slug. */
  readonly encodeSlug: (cwdPath: string) => string;
  /** Resolve the git repo root for a dir, falling back to the dir itself. Never fails. */
  readonly gitRoot: (cwd: string) => Effect.Effect<string>;
  /** Enumerate every project slug for an agent (all projects). */
  readonly listProjectSlugs: (
    agent: AgentId
  ) => Effect.Effect<readonly string[], AgentUnsupportedError, never>;
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
      requireSupported(agent, "memoryDir").pipe(
        Effect.as(join(ROOTS()[agent].projectsRoot, slug, "memory"))
      );

    const listProjectSlugs = (agent: AgentId) =>
      requireSupported(agent, "listProjectSlugs").pipe(
        Effect.flatMap(() => {
          const root = ROOTS()[agent].projectsRoot;
          return fs.exists(root).pipe(
            Effect.flatMap((present) =>
              present
                ? fs.readDirectory(root)
                : Effect.succeed<readonly string[]>([])
            ),
            Effect.flatMap((names) =>
              Effect.forEach(names, (name) =>
                fs.stat(join(root, name)).pipe(
                  Effect.map((info) =>
                    info.type === "Directory" ? name : null
                  ),
                  Effect.orElseSucceed(() => null)
                )
              )
            ),
            Effect.map((names) =>
              names.filter((name): name is string => name !== null)
            ),
            Effect.orElseSucceed(() => [] as readonly string[])
          );
        }),
        Effect.withSpan("AgentRegistry.listProjectSlugs", {
          attributes: { agent },
        })
      );

    return {
      encodeSlug,
      roots: (agent) => ROOTS()[agent],
      allowedRoots,
      gitRoot,
      projectsRoot,
      listProjectSlugs,
      sessionsGlob,
      memoryDir,
    } satisfies AgentRegistryShape;
  })
);
