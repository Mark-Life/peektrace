/** Locate a transcript by id and gather companion files via the platform FS. */
import type { FileSystem } from "@effect/platform";
import { Effect, Option } from "effect";
import { type AgentRegistryShape, sourcesOf } from "../agents";
import { capMemory, projectContext } from "./context-dirs";
import { rulePaths } from "./context-rules";
import { SessionNotFoundError } from "./errors";
import type { OnDiskContextFile, SubagentRef } from "./schema";
import { estTokens } from "./tokens";
import { parseWorkflowRun, toAgentRef, type WorkflowRun } from "./workflows";

const BYTES_PER_TOKEN = 4;
const SUBAGENT_FILE = /^agent-.*\.jsonl$/;
const JSONL_EXT = /\.jsonl$/;
const SLASHES = /\/+/g;
const WORKFLOW_RUN = "wf_";

/** A resolved transcript location plus its optional subagents dir. */
export interface ResolvedSession {
  readonly path: string;
  readonly projectDir: string;
  readonly sessionId: string;
  readonly subagentDir?: string;
}

/** A subagent transcript discovered on disk, before its turns are parsed. */
export interface SubagentStub
  extends Omit<SubagentRef, "turns" | "peakContextTokens"> {
  /** Workflow agent start, epoch ms; picks the call among resumes of its run. */
  readonly workflowStartedAt?: number;
}

const join = (...parts: readonly string[]): string =>
  parts.join("/").replace(SLASHES, "/");

const basename = (path: string): string => path.split("/").pop() ?? path;

const dirname = (path: string): string =>
  path.split("/").slice(0, -1).join("/");

/** Workflow fields from meta + run json (empty outside a workflow run). */
const workflowFields = (args: {
  readonly id: string;
  readonly meta: Record<string, unknown>;
  readonly run?: WorkflowRun;
}) => {
  const { id, meta, run } = args;
  if (!run) {
    return {};
  }
  const phase = run.agents.get(id);
  const title =
    typeof meta.workflowPhase === "string"
      ? meta.workflowPhase
      : phase?.phaseTitle;
  return {
    workflowRunId: run.runId,
    ...(title ? { workflowPhase: title } : {}),
    ...(phase?.phaseIndex === undefined
      ? {}
      : { workflowPhaseIndex: phase.phaseIndex }),
    ...(phase?.startedAt === undefined
      ? {}
      : { workflowStartedAt: phase.startedAt }),
  };
};

/** Debug-log a read that falls back to an empty value instead of failing. */
const logSkipped = (args: {
  readonly what: string;
  readonly path: string;
  readonly e: unknown;
}) =>
  Effect.logDebug(`${args.what} unreadable`).pipe(
    Effect.annotateLogs({ path: args.path, reason: String(args.e) })
  );

/** Build a subagent stub from its path + parsed meta (no control flow). */
const buildStub = (args: {
  readonly id: string;
  readonly path: string;
  readonly meta: Record<string, unknown>;
  readonly run?: WorkflowRun;
}): SubagentStub => {
  const { id, path, meta } = args;
  return {
    id,
    path,
    ...(typeof meta.agentType === "string"
      ? { agentType: meta.agentType }
      : {}),
    ...(typeof meta.description === "string"
      ? { description: meta.description }
      : {}),
    ...(typeof meta.toolUseId === "string"
      ? { toolUseId: meta.toolUseId }
      : {}),
    ...(typeof meta.parentAgentId === "string"
      ? { parentAgentId: toAgentRef(meta.parentAgentId) }
      : {}),
    ...workflowFields(args),
  };
};

/** A transcript whose filename starts with the requested short id. */
interface PrefixMatch {
  readonly projectDir: string;
  readonly sessionId: string;
}

/** Scan every project dir for transcripts whose id starts with `prefix`. */
const findByPrefix = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly projectsRoot: string;
  readonly slugs: readonly string[];
  readonly prefix: string;
}): Effect.Effect<PrefixMatch[]> => {
  const { fs, projectsRoot, slugs, prefix } = args;
  return Effect.gen(function* () {
    const matches: PrefixMatch[] = [];
    for (const slug of slugs) {
      const projectDir = join(projectsRoot, slug);
      const names = yield* fs
        .readDirectory(projectDir)
        .pipe(Effect.orElseSucceed(() => [] as string[]));
      for (const name of names) {
        const sessionId = name.replace(JSONL_EXT, "");
        if (JSONL_EXT.test(name) && sessionId.startsWith(prefix)) {
          matches.push({ sessionId, projectDir });
        }
      }
    }
    return matches;
  });
};

/** First source projects-root under which `<slug>/<id>.jsonl` exists, else null. */
const findExactAcrossSources = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly sources: readonly { readonly projectsRoot: string }[];
  readonly slugs: readonly string[];
  readonly id: string;
}): Effect.Effect<string | null> => {
  const { fs, sources, slugs, id } = args;
  return Effect.gen(function* () {
    for (const source of sources) {
      for (const slug of slugs) {
        const projectDir = join(source.projectsRoot, slug);
        const found = yield* fs
          .exists(join(projectDir, `${id}.jsonl`))
          .pipe(Effect.orElseSucceed(() => false));
        if (found) {
          return projectDir;
        }
      }
    }
    return null;
  });
};

/** Prefix matches gathered across every source root (for global uniqueness). */
const findPrefixAcrossSources = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly sources: readonly { readonly projectsRoot: string }[];
  readonly slugs: readonly string[];
  readonly prefix: string;
}): Effect.Effect<PrefixMatch[]> => {
  const { fs, sources, slugs, prefix } = args;
  return Effect.gen(function* () {
    const all: PrefixMatch[] = [];
    for (const source of sources) {
      const matches = yield* findByPrefix({
        fs,
        projectsRoot: source.projectsRoot,
        slugs,
        prefix,
      });
      all.push(...matches);
    }
    return all;
  });
};

/** Resolve a Claude session id (or direct .jsonl path) to its transcript. */
export const resolveClaudeSession = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly idOrPath: string;
}): Effect.Effect<ResolvedSession, SessionNotFoundError> => {
  const { fs, agents, idOrPath } = args;
  return Effect.gen(function* () {
    const sources = sourcesOf(agents.roots("claude"));
    const projectsRoot = sources[0]?.projectsRoot ?? "";

    const withSubagentDir = (sessionId: string, projectDir: string) =>
      Effect.gen(function* () {
        const dir = join(projectDir, sessionId, "subagents");
        const present = yield* fs
          .exists(dir)
          .pipe(Effect.orElseSucceed(() => false));
        return {
          sessionId,
          path: join(projectDir, `${sessionId}.jsonl`),
          projectDir,
          ...(present ? { subagentDir: dir } : {}),
        } satisfies ResolvedSession;
      });

    if (idOrPath.endsWith(".jsonl")) {
      const direct = yield* fs
        .exists(idOrPath)
        .pipe(Effect.orElseSucceed(() => false));
      if (direct) {
        const sessionId = basename(idOrPath).replace(JSONL_EXT, "");
        const projectDir = idOrPath.slice(0, idOrPath.lastIndexOf("/"));
        const dir = join(projectDir, sessionId, "subagents");
        const present = yield* fs
          .exists(dir)
          .pipe(Effect.orElseSucceed(() => false));
        return {
          sessionId,
          path: idOrPath,
          projectDir,
          ...(present ? { subagentDir: dir } : {}),
        } satisfies ResolvedSession;
      }
    }

    const id = idOrPath.replace(JSONL_EXT, "");
    const slugs = yield* agents.listProjectSlugs("claude").pipe(
      Effect.tapError((e) =>
        logSkipped({ what: "project slugs", path: projectsRoot, e })
      ),
      Effect.orElseSucceed(() => [] as readonly string[])
    );
    // A slug only exists under its own root, so probing the union of slugs
    // against every source root is filtered by `exists`.
    const exactDir = yield* findExactAcrossSources({ fs, sources, slugs, id });
    if (exactDir) {
      return yield* withSubagentDir(id, exactDir);
    }

    // Fallback: treat `id` as a short prefix (as printed by `sessions ls`) and
    // resolve it only when it uniquely identifies one transcript across ALL
    // roots, so a prefix that is ambiguous across accounts is reported not-found
    // rather than silently picking whichever source is scanned first.
    const allMatches = yield* findPrefixAcrossSources({
      fs,
      sources,
      slugs,
      prefix: id,
    });
    const [match] = allMatches;
    if (allMatches.length === 1 && match) {
      return yield* withSubagentDir(match.sessionId, match.projectDir);
    }

    return yield* Effect.fail(
      new SessionNotFoundError({ id, searchedRoot: projectsRoot })
    );
  }).pipe(Effect.withSpan("Sessions.resolve", { attributes: { idOrPath } }));
};

/** Recursively collect `agent-*.jsonl` stubs under a subagents dir. */
export const findSubagents = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly subagentDir?: string;
}): Effect.Effect<SubagentStub[]> => {
  const { fs, subagentDir } = args;
  if (!subagentDir) {
    return Effect.succeed([]);
  }

  const readMeta = (dir: string, id: string) =>
    fs.readFileString(join(dir, `${id}.meta.json`)).pipe(
      Effect.flatMap((raw) =>
        Effect.try(() => JSON.parse(raw) as Record<string, unknown>)
      ),
      Effect.tapError((e) =>
        logSkipped({ what: "subagent meta", path: join(dir, id), e })
      ),
      Effect.orElseSucceed(() => ({}) as Record<string, unknown>)
    );

  const workflowsDir = join(subagentDir, "workflows");
  const readRun = (runId: string): Effect.Effect<WorkflowRun> =>
    fs
      .readFileString(join(dirname(subagentDir), "workflows", `${runId}.json`))
      .pipe(
        Effect.map((raw) => ({ runId, ...parseWorkflowRun(raw) })),
        Effect.tapError((e) =>
          logSkipped({ what: "workflow run", path: runId, e })
        ),
        Effect.orElseSucceed(() => ({ runId, agents: new Map() }))
      );

  const processEntry = (
    dir: string,
    name: string,
    run?: WorkflowRun
  ): Effect.Effect<SubagentStub[]> =>
    Effect.gen(function* () {
      const path = join(dir, name);
      const type = yield* fs.stat(path).pipe(
        Effect.map((s) => s.type),
        Effect.tapError((e) => logSkipped({ what: "subagent entry", path, e })),
        Effect.orElseSucceed(() => "Other" as const)
      );
      if (type === "Directory") {
        const isRun = dir === workflowsDir && name.startsWith(WORKFLOW_RUN);
        return yield* walk(path, isRun ? yield* readRun(name) : run);
      }
      if (type === "File" && SUBAGENT_FILE.test(name)) {
        const id = name.replace(JSONL_EXT, "");
        const meta = yield* readMeta(dir, id);
        return [buildStub({ id, path, meta, ...(run ? { run } : {}) })];
      }
      return [];
    });

  function walk(dir: string, run?: WorkflowRun): Effect.Effect<SubagentStub[]> {
    return fs.readDirectory(dir).pipe(
      Effect.tapError((e) =>
        logSkipped({ what: "subagent dir", path: dir, e })
      ),
      Effect.orElseSucceed(() => [] as string[]),
      Effect.flatMap((names) =>
        Effect.forEach(names, (name) => processEntry(dir, name, run))
      ),
      Effect.map((lists) => lists.flat())
    );
  }

  return fs.exists(subagentDir).pipe(
    Effect.orElseSucceed(() => false),
    Effect.flatMap((present) =>
      present ? walk(subagentDir) : Effect.succeed([])
    )
  );
};

/** One context file as it sits on disk, symlinks resolved. */
interface ContextRead {
  readonly bytes: number;
  /** Last modification, epoch ms; null when the platform reports none. */
  readonly mtimeMs: number | null;
  /** The path with every symlink resolved. */
  readonly realPath: string;
  /** Null when the file exists but its text could not be read. */
  readonly text: string | null;
}

/**
 * Stat and read one context file through the platform FS, following
 * symlinks, or null when it is absent or cannot be stat'd.
 */
const readContext = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly path: string;
}): Effect.Effect<ContextRead | null> => {
  const { fs, path } = args;
  return fs.exists(path).pipe(
    Effect.orElseSucceed(() => false),
    Effect.flatMap((present) =>
      present
        ? Effect.gen(function* () {
            const realPath = yield* fs
              .realPath(path)
              .pipe(Effect.orElseSucceed(() => path));
            const info = yield* fs.stat(realPath);
            const text = yield* fs
              .readFileString(realPath)
              .pipe(Effect.orElseSucceed(() => null));
            const mtime = Option.getOrNull(info.mtime);
            return {
              bytes: Number(info.size),
              text,
              realPath,
              mtimeMs: mtime === null ? null : mtime.getTime(),
            } satisfies ContextRead;
          }).pipe(Effect.orElseSucceed(() => null))
        : Effect.succeed(null)
    )
  );
};

/**
 * One memory file's text, where it really lives and when it last changed, or
 * null when it is absent or unreadable.
 */
export const readContextText = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly path: string;
}): Effect.Effect<(ContextRead & { readonly text: string }) | null> =>
  readContext(args).pipe(
    Effect.map((read) =>
      read === null || read.text === null ? null : { ...read, text: read.text }
    )
  );

/** Read one context file into an OnDiskContextFile, or null when absent. */
const readContextFile = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly path: string;
  readonly label: string;
  readonly scope: OnDiskContextFile["scope"];
}): Effect.Effect<OnDiskContextFile | null> => {
  const { path, label, scope } = args;
  return readContext(args).pipe(
    Effect.map((read) =>
      read === null
        ? null
        : ({
            label,
            path,
            bytes: read.bytes,
            tokensEst: read.text
              ? estTokens(read.text)
              : Math.round(read.bytes / BYTES_PER_TOKEN),
            scope,
          } satisfies OnDiskContextFile)
    )
  );
};

/** Resolve the home + projectsRoot of the Claude source owning a session id. */
const claudeSourcePaths = (
  agents: AgentRegistryShape,
  source?: string
): { readonly home: string; readonly projectsRoot: string } => {
  const claudeSources = sourcesOf(agents.roots("claude"));
  const owning =
    (source ? claudeSources.find((s) => s.id === source) : undefined) ??
    claudeSources[0];
  const fallback = agents.roots("claude");
  return {
    home: owning?.home ?? fallback.home,
    projectsRoot: owning?.projectsRoot ?? fallback.projectsRoot,
  };
};

/**
 * Gather the CLAUDE.md / AGENTS.md / CLAUDE.local.md / rules / MEMORY.md files
 * Claude Code injects into the system prompt for `cwd`, used to attribute the
 * system+tools residual.
 */
export const gatherOnDiskContextFiles = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cwd?: string;
  /** The session's resolved source id, so context is read from the account that
   * actually owns it rather than always the default. */
  readonly source?: string;
}): Effect.Effect<OnDiskContextFile[]> => {
  const { fs, agents, cwd, source } = args;
  return Effect.gen(function* () {
    // Read from the account that actually owns the session (falls back to default).
    const { home, projectsRoot } = claudeSourcePaths(agents, source);
    const files: OnDiskContextFile[] = [];
    const push = (f: OnDiskContextFile | null) => {
      if (f) {
        files.push(f);
      }
    };

    push(
      yield* readContextFile({
        fs,
        path: join(home, "CLAUDE.md"),
        label: "~/.claude/CLAUDE.md (global)",
        scope: "global",
      })
    );

    for (const path of yield* rulePaths({ fs, dir: join(home, "rules") })) {
      push(yield* readContextFile({ fs, path, label: path, scope: "global" }));
    }

    if (cwd) {
      const project = yield* projectContext({ fs, cwd, home });
      for (const path of project.paths) {
        push(
          yield* readContextFile({ fs, path, label: path, scope: "project" })
        );
      }

      const memPath = join(
        projectsRoot,
        agents.encodeSlug(project.memoryRoot),
        "memory",
        "MEMORY.md"
      );
      const mem = yield* readContextText({ fs, path: memPath });
      if (mem) {
        const text = capMemory(mem.text);
        files.push({
          label: "memory/MEMORY.md",
          path: memPath,
          bytes: new TextEncoder().encode(text).length,
          tokensEst: estTokens(text),
          scope: "memory",
        });
      }
    }

    return files;
  }).pipe(Effect.withSpan("Sessions.gatherContextFiles"));
};
