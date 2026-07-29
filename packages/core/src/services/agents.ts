import { execFileSync } from "node:child_process";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { FileSystem } from "@effect/platform";
import { Context, Data, Effect, Layer, Option } from "effect";
import { AGENT_IDS, type AgentId } from "./agent-id";
import {
  listSessionRefs,
  loadSessionText,
  resolveDataDir,
  type SessionText,
} from "./sessions/opencode/reader";
import {
  loadSettings,
  type PeektraceSettings,
  type RootEntry,
} from "./settings";

export { AGENT_IDS, AgentId } from "./agent-id";

/** How an agent lays its transcripts out on disk. Drives `listSessionFiles`. */
export type SessionLayout =
  /** `~/.claude/projects/<slug>/<id>.jsonl` (+ `<id>/subagents/…`). */
  | "claude-projects"
  /** `~/.codex/sessions/YYYY/MM/DD/rollout-<iso>-<uuid>.jsonl` (date tree). */
  | "codex-datetree"
  /** `~/.pi/agent/sessions/<cwd-slug>/<iso>_<uuid>.jsonl`. */
  | "pi-cwd-slug"
  /** OpenCode SQLite DB (+ legacy JSON tree) under `~/.local/share/opencode`. */
  | "opencode-sqlite"
  /** No parseable session layout yet. */
  | "none";

/** One on-disk source (config dir) an agent's transcripts are read from. */
export interface AgentSource {
  /** This source's config/home dir. */
  readonly home: string;
  /** Stable id: the agent id for the default, `<agent>:<label>` for extras. */
  readonly id: string;
  /** True for the env/default-resolved source (there is exactly one). */
  readonly isDefault: boolean;
  /** Human label shown in the UI badge/facet. */
  readonly label: string;
  /** This source's session-transcript root. */
  readonly projectsRoot: string;
}

/** On-disk roots declared for an agent. */
export interface AgentRoots {
  /** Base config dir of the default source, e.g. ~/.claude. */
  readonly home: string;
  readonly id: AgentId;
  /** On-disk transcript layout; selects the `listSessionFiles` walker. */
  readonly layout: SessionLayout;
  /** Transcript root of the default source, e.g. ~/.claude/projects. */
  readonly projectsRoot: string;
  /**
   * Every source scanned for this agent (default first, then any declared in
   * the user config). Optional so lightweight test stubs may omit it; read it
   * through `sourcesOf`, which falls back to the single default root.
   */
  readonly sources?: readonly AgentSource[];
  /** True for agents whose transcripts can be listed + parsed (claude/codex/pi). */
  readonly supported: boolean;
}

/**
 * The sources to scan for an agent: its declared `sources`, or a single default
 * synthesized from the scalar `home`/`projectsRoot` when a stub omits them.
 */
export const sourcesOf = (roots: AgentRoots): readonly AgentSource[] =>
  roots.sources ?? [
    {
      id: roots.id,
      label: "default",
      home: roots.home,
      projectsRoot: roots.projectsRoot,
      isDefault: true,
    },
  ];

/** One transcript located on disk, before its body is parsed. */
export interface SessionFileRef {
  /** Stable session id (filename stem for Claude/Pi, rollout uuid for Codex). */
  readonly id: string;
  /** Absolute path to the `.jsonl` transcript. */
  readonly path: string;
  /** Owning project slug, or `""` when the layout has none (Codex date tree). */
  readonly slug: string;
  /** Owning source (id + label), set only when the agent has >1 source. */
  readonly source?: { readonly id: string; readonly label: string };
}

/** Raised when a resolver is invoked for an agent that is declared but unimplemented. */
export class AgentUnsupportedError extends Data.TaggedError(
  "AgentUnsupportedError"
)<{
  readonly agent: AgentId;
  readonly operation: string;
}> {}

/** Raised when a transcript cannot be read (file IO or SQLite/tree read). */
export class TranscriptReadError extends Data.TaggedError(
  "TranscriptReadError"
)<{
  readonly path: string;
  readonly reason: string;
}> {}

/** A transcript's raw text plus the stat used for header sizing/recency. */
export type TranscriptPayload = SessionText;

/**
 * First non-empty entry of a `PATH`-style env var. Claude Code accepts a
 * colon-separated list in `CLAUDE_CONFIG_DIR`; peektrace scans a single root, so
 * we take the first and ignore the rest (see issue #21 open question).
 */
const firstDir = (value: string | undefined): string | undefined =>
  value
    ?.split(":")
    .map((part) => part.trim())
    .find((part) => part.length > 0);

/** Expand a leading `~` / `~/` in a user-configured path to the home dir. */
const expandHome = (path: string, home: string): string => {
  if (path === "~") {
    return home;
  }
  return path.startsWith("~/") ? join(home, path.slice(2)) : path;
};

/** Derive an agent's transcript root from a config HOME dir, per layout. */
const deriveProjectsRoot = (layout: SessionLayout, home: string): string => {
  switch (layout) {
    case "claude-projects":
      return join(home, "projects");
    case "codex-datetree":
      return join(home, "sessions");
    case "pi-cwd-slug":
      return join(home, "agent", "sessions");
    default:
      // opencode-sqlite (home === dataDir) and "none".
      return home;
  }
};

/**
 * Build the full source list for an agent: its env/default source first, then
 * any user-configured extras, deduped by resolved transcript root (default wins)
 * with stable, unique ids.
 */
const buildSources = (
  roots: AgentRoots,
  configured: readonly RootEntry[],
  home: string
): readonly AgentSource[] => {
  const sources: AgentSource[] = [
    {
      id: roots.id,
      label: "default",
      home: roots.home,
      projectsRoot: roots.projectsRoot,
      isDefault: true,
    },
  ];
  const seenPaths = new Set<string>([roots.projectsRoot]);
  // Labels must be unique: the UI keys the source badge/facet on the label, so
  // two roots sharing one (or colliding with "default") would otherwise merge.
  const usedLabels = new Set<string>(["default"]);
  for (const entry of configured) {
    const srcHome = expandHome(entry.path.trim(), home);
    if (srcHome === "") {
      continue;
    }
    const projectsRoot = deriveProjectsRoot(roots.layout, srcHome);
    if (seenPaths.has(projectsRoot)) {
      continue;
    }
    seenPaths.add(projectsRoot);
    const base = entry.label?.trim() || basename(srcHome) || "source";
    let label = base;
    for (let n = 2; usedLabels.has(label); n++) {
      label = `${base}-${n}`;
    }
    usedLabels.add(label);
    sources.push({
      id: `${roots.id}:${label}`,
      label,
      home: srcHome,
      projectsRoot,
      isDefault: false,
    });
  }
  return sources;
};

/**
 * Compute the declared roots for every agent from an env + home pair (plus the
 * optional user config). Pure and exported so tests can exercise the resolution
 * without touching the process env or the memoized cache.
 *
 * Each agent's default source resolves from its own native override env var
 * (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_DATA_HOME` for OpenCode), falling back
 * to the default `~/.<agent>` location. The `PEEKTRACE_*` vars remain internal
 * test hooks and, where present, still win over everything for the default
 * source's projects root. Extra roots declared in `settings.roots` are appended
 * as additional sources so a user can scan several config dirs (e.g. a separate
 * work account) in parallel; `sources` is set only when more than one exists.
 */
export const computeRoots = (
  env: NodeJS.ProcessEnv,
  home: string,
  settings: PeektraceSettings = {}
): Record<AgentId, AgentRoots> => {
  const claudeHome = firstDir(env.CLAUDE_CONFIG_DIR) ?? join(home, ".claude");
  const codexHome = env.CODEX_HOME ?? join(home, ".codex");
  const base: Record<AgentId, AgentRoots> = {
    claude: {
      id: "claude",
      home: claudeHome,
      layout: "claude-projects",
      projectsRoot:
        env.PEEKTRACE_CLAUDE_PROJECTS ?? join(claudeHome, "projects"),
      supported: true,
    },
    codex: {
      id: "codex",
      home: codexHome,
      layout: "codex-datetree",
      projectsRoot: env.PEEKTRACE_CODEX_SESSIONS ?? join(codexHome, "sessions"),
      supported: true,
    },
    pi: {
      id: "pi",
      home: join(home, ".pi"),
      layout: "pi-cwd-slug",
      projectsRoot:
        env.PEEKTRACE_PI_SESSIONS ?? join(home, ".pi", "agent", "sessions"),
      supported: true,
    },
    opencode: {
      id: "opencode",
      home: resolveDataDir(env),
      layout: "opencode-sqlite",
      projectsRoot: resolveDataDir(env),
      supported: true,
    },
  };
  const withSources = (roots: AgentRoots): AgentRoots => {
    const sources = buildSources(roots, settings.roots?.[roots.id] ?? [], home);
    return sources.length > 1 ? { ...roots, sources } : roots;
  };
  return {
    claude: withSources(base.claude),
    codex: withSources(base.codex),
    pi: withSources(base.pi),
    opencode: withSources(base.opencode),
  };
};

/**
 * Build the declared roots for every agent. Computed lazily (and memoized) so
 * importing this module never touches Node's `os`/`path` or `process.env` — the
 * RPC contract pulls these schemas into the browser bundle, where those builtins
 * are stubbed and would throw at import time if invoked eagerly. This config-less
 * variant backs the pure `supported`/`layout` gates; the live layer rebuilds the
 * roots with the user config folded in.
 */
const buildRoots = (): Record<AgentId, AgentRoots> =>
  computeRoots(process.env, homedir());

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
  /**
   * Load one transcript's text + stat, abstracting over file-backed agents
   * (Claude/Codex/Pi read `ref.path` off disk) and OpenCode (whose `ref.path`
   * is a `<dataDir>#<sessionId>` handle the SQLite/tree reader serializes).
   */
  readonly loadTranscript: (args: {
    readonly agent: AgentId;
    readonly ref: SessionFileRef;
  }) => Effect.Effect<TranscriptPayload, TranscriptReadError>;
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

/** Path to the optional user settings file, under `PEEKTRACE_DIR` or `~/.peektrace`. */
export const settingsPath = (): string =>
  join(
    process.env.PEEKTRACE_DIR ?? join(homedir(), ".peektrace"),
    "settings.json"
  );

/** Live layer: resolves agent paths via the platform FileSystem, folding in the
 * user settings so extra roots (e.g. a second Claude account) are scanned too. */
export const AgentRegistryLive = Layer.effect(
  AgentRegistry,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    // Settings-aware roots, built once at layer construction. Node IO happens
    // here (server-side), never at import — the browser only pulls the schemas.
    const settings = yield* loadSettings(fs, settingsPath());
    const roots = computeRoots(process.env, homedir(), settings);
    const rootOf = (agent: AgentId): AgentRoots => roots[agent];

    const allowedRoots = AGENT_IDS.flatMap((id) =>
      sourcesOf(rootOf(id)).flatMap((s) => [s.home, s.projectsRoot])
    );

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
        Effect.as(rootOf(agent).projectsRoot)
      );

    const sessionsGlob = (agent: AgentId) =>
      requireSupported(agent, "sessionsGlob").pipe(
        Effect.as(join(rootOf(agent).projectsRoot, "**", "*.jsonl"))
      );

    /** The memory dir for a slug: the source whose projects root actually holds
     * it (so a second account's slug resolves correctly), else the default. */
    const memoryDir = ({
      agent,
      slug,
    }: {
      readonly agent: AgentId;
      readonly slug: string;
    }) =>
      requireClaudeLayout(agent, "memoryDir").pipe(
        Effect.flatMap(() =>
          Effect.reduce(
            sourcesOf(rootOf(agent)),
            null as string | null,
            (found, source) =>
              found
                ? Effect.succeed(found)
                : fs.exists(join(source.projectsRoot, slug)).pipe(
                    Effect.orElseSucceed(() => false),
                    Effect.map((present) =>
                      present ? source.projectsRoot : null
                    )
                  )
          )
        ),
        Effect.map((root) =>
          join(root ?? rootOf(agent).projectsRoot, slug, "memory")
        )
      );

    const listProjectSlugs = (agent: AgentId) =>
      requireClaudeLayout(agent, "listProjectSlugs").pipe(
        Effect.flatMap(() =>
          Effect.forEach(sourcesOf(rootOf(agent)), (source) =>
            listSlugDirs(fs, source.projectsRoot)
          )
        ),
        Effect.map((groups) => [...new Set(groups.flat())]),
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
      const { layout } = rootOf(agent);
      const sources = sourcesOf(rootOf(agent));
      // Only stamp a source when there's more than one — single-account users
      // get no badge/facet noise.
      const stamp = sources.length > 1;
      const buildFor = (
        root: string
      ): Effect.Effect<readonly SessionFileRef[]> => {
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
          case "opencode-sqlite":
            return Effect.sync(() =>
              listSessionRefs(root).map(
                (ref) =>
                  ({
                    id: ref.id,
                    slug: ref.directory ?? "",
                    path: `${root}#${ref.id}`,
                  }) satisfies SessionFileRef
              )
            );
          default:
            return Effect.succeed([] as readonly SessionFileRef[]);
        }
      };
      return Effect.forEach(sources, (source) =>
        buildFor(source.projectsRoot).pipe(
          Effect.map((refs) =>
            stamp
              ? refs.map((ref) => ({
                  ...ref,
                  source: { id: source.id, label: source.label },
                }))
              : refs
          ),
          Effect.orElseSucceed(() => [] as readonly SessionFileRef[])
        )
      ).pipe(
        // Dedup by transcript path so overlapping roots never double-count.
        Effect.map((groups) => {
          const seen = new Set<string>();
          const out: SessionFileRef[] = [];
          for (const ref of groups.flat()) {
            if (!seen.has(ref.path)) {
              seen.add(ref.path);
              out.push(ref);
            }
          }
          return out as readonly SessionFileRef[];
        }),
        Effect.orElseSucceed(() => [] as readonly SessionFileRef[]),
        Effect.withSpan("AgentRegistry.listSessionFiles", {
          attributes: { agent },
        })
      );
    };

    /** OpenCode: `<dataDir>#<sessionId>` → serialized dialect via the reader. */
    const loadOpencodeTranscript = (ref: SessionFileRef) =>
      Effect.try({
        try: () => {
          // Split on the LAST `#`: OpenCode session ids (`ses_<base62>`) never
          // contain `#`, so lastIndexOf lands on the `<dataDir>#<id>` delimiter.
          const hash = ref.path.lastIndexOf("#");
          const dataDir =
            hash >= 0
              ? ref.path.slice(0, hash)
              : rootOf("opencode").projectsRoot;
          const sessionId = hash >= 0 ? ref.path.slice(hash + 1) : ref.id;
          return loadSessionText(
            dataDir,
            sessionId
          ) satisfies TranscriptPayload;
        },
        catch: (e) =>
          new TranscriptReadError({ path: ref.path, reason: String(e) }),
      });

    /** File-backed agents (Claude/Codex/Pi): stat + read `ref.path` off disk. */
    const loadFileTranscript = (ref: SessionFileRef) =>
      Effect.gen(function* () {
        const info = yield* fs.stat(ref.path);
        const text = yield* fs.readFileString(ref.path);
        const mtimeMs = Option.match(info.mtime, {
          onNone: () => 0,
          onSome: (d) => d.getTime(),
        });
        return {
          text,
          sizeBytes: Number(info.size),
          mtimeMs,
        } satisfies TranscriptPayload;
      }).pipe(
        Effect.mapError(
          (e) => new TranscriptReadError({ path: ref.path, reason: String(e) })
        )
      );

    const loadTranscript = ({
      agent,
      ref,
    }: {
      readonly agent: AgentId;
      readonly ref: SessionFileRef;
    }) =>
      (rootOf(agent).layout === "opencode-sqlite"
        ? loadOpencodeTranscript(ref)
        : loadFileTranscript(ref)
      ).pipe(
        Effect.withSpan("AgentRegistry.loadTranscript", {
          attributes: { agent },
        })
      );

    return {
      encodeSlug,
      roots: rootOf,
      allowedRoots,
      gitRoot,
      projectsRoot,
      listProjectSlugs,
      listSessionFiles,
      loadTranscript,
      sessionsGlob,
      memoryDir,
    } satisfies AgentRegistryShape;
  })
);
