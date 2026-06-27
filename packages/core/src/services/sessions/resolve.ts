/** Locate a transcript by id and gather companion files via the platform FS. */
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import type { AgentRegistryShape } from "../agents";
import { SessionNotFoundError } from "./errors";
import type { OnDiskContextFile, SubagentRef } from "./schema";
import { estTokens } from "./tokens";

const BYTES_PER_TOKEN = 4;
const SUBAGENT_FILE = /^agent-.*\.jsonl$/;
const JSONL_EXT = /\.jsonl$/;
const SLASHES = /\/+/g;

/** A resolved transcript location plus its optional subagents dir. */
export interface ResolvedSession {
  readonly path: string;
  readonly projectDir: string;
  readonly sessionId: string;
  readonly subagentDir?: string;
}

/** A subagent transcript discovered on disk, before its turns are parsed. */
export type SubagentStub = Omit<SubagentRef, "turns" | "peakContextTokens">;

const join = (...parts: readonly string[]): string =>
  parts.join("/").replace(SLASHES, "/");

const basename = (path: string): string => path.split("/").pop() ?? path;

/** Build a subagent stub from its path + parsed meta (no control flow). */
const buildStub = (args: {
  readonly id: string;
  readonly path: string;
  readonly meta: Record<string, unknown>;
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
  };
};

/** Resolve a Claude session id (or direct .jsonl path) to its transcript. */
export const resolveClaudeSession = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly idOrPath: string;
}): Effect.Effect<ResolvedSession, SessionNotFoundError> => {
  const { fs, agents, idOrPath } = args;
  return Effect.gen(function* () {
    const projectsRoot = agents.roots("claude").projectsRoot;

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
    const slugs = yield* agents
      .listProjectSlugs("claude")
      .pipe(Effect.orElseSucceed(() => [] as readonly string[]));
    for (const slug of slugs) {
      const projectDir = join(projectsRoot, slug);
      const candidate = join(projectDir, `${id}.jsonl`);
      const found = yield* fs
        .exists(candidate)
        .pipe(Effect.orElseSucceed(() => false));
      if (found) {
        return yield* withSubagentDir(id, projectDir);
      }
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
      Effect.map((raw) => JSON.parse(raw) as Record<string, unknown>),
      Effect.orElseSucceed(() => ({}) as Record<string, unknown>)
    );

  const processEntry = (
    dir: string,
    name: string
  ): Effect.Effect<SubagentStub[]> =>
    Effect.gen(function* () {
      const path = join(dir, name);
      const type = yield* fs.stat(path).pipe(
        Effect.map((s) => s.type),
        Effect.orElseSucceed(() => "Other" as const)
      );
      if (type === "Directory") {
        return yield* walk(path);
      }
      if (type === "File" && SUBAGENT_FILE.test(name)) {
        const id = name.replace(JSONL_EXT, "");
        const meta = yield* readMeta(dir, id);
        return [buildStub({ id, path, meta })];
      }
      return [];
    });

  function walk(dir: string): Effect.Effect<SubagentStub[]> {
    return fs.readDirectory(dir).pipe(
      Effect.orElseSucceed(() => [] as string[]),
      Effect.flatMap((names) =>
        Effect.forEach(names, (name) => processEntry(dir, name))
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

/** Read one context file into an OnDiskContextFile, or null when absent. */
const readContextFile = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly path: string;
  readonly label: string;
  readonly scope: OnDiskContextFile["scope"];
}): Effect.Effect<OnDiskContextFile | null> => {
  const { fs, path, label, scope } = args;
  return fs.exists(path).pipe(
    Effect.orElseSucceed(() => false),
    Effect.flatMap((present) =>
      present
        ? Effect.gen(function* () {
            const info = yield* fs.stat(path);
            const bytes = Number(info.size);
            const text = yield* fs
              .readFileString(path)
              .pipe(Effect.orElseSucceed(() => ""));
            return {
              label,
              path,
              bytes,
              tokensEst: text
                ? estTokens(text)
                : Math.round(bytes / BYTES_PER_TOKEN),
              scope,
            } satisfies OnDiskContextFile;
          }).pipe(Effect.orElseSucceed(() => null))
        : Effect.succeed(null)
    )
  );
};

/** Sum byte sizes of every file under a memory dir, returning [bytes, count]. */
const sumMemoryDir = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly dir: string;
}): Effect.Effect<readonly [number, number]> => {
  const { fs, dir } = args;
  const walk = (d: string): Effect.Effect<readonly [number, number]> =>
    Effect.gen(function* () {
      const names = yield* fs
        .readDirectory(d)
        .pipe(Effect.orElseSucceed(() => [] as string[]));
      let bytes = 0;
      let count = 0;
      for (const name of names) {
        const path = join(d, name);
        const info = yield* fs.stat(path).pipe(
          Effect.map((s) => s),
          Effect.orElseSucceed(() => null)
        );
        if (!info) {
          continue;
        }
        if (info.type === "Directory") {
          const [b, c] = yield* walk(path);
          bytes += b;
          count += c;
        } else if (info.type === "File") {
          bytes += Number(info.size);
          count += 1;
        }
      }
      return [bytes, count] as const;
    });
  return walk(dir);
};

/**
 * Gather the CLAUDE.md / AGENTS.md / memory files that would be injected into
 * the system prompt for `cwd`, used to attribute the system+tools residual.
 */
export const gatherOnDiskContextFiles = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cwd?: string;
}): Effect.Effect<OnDiskContextFile[]> => {
  const { fs, agents, cwd } = args;
  return Effect.gen(function* () {
    const home = agents.roots("claude").home;
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

    if (cwd) {
      const projectFiles: ReadonlyArray<readonly [string, string]> = [
        [join(cwd, "CLAUDE.md"), `${cwd}/CLAUDE.md`],
        [join(cwd, "AGENTS.md"), `${cwd}/AGENTS.md`],
        [join(cwd, ".claude", "CLAUDE.md"), `${cwd}/.claude/CLAUDE.md`],
      ];
      for (const [path, label] of projectFiles) {
        push(yield* readContextFile({ fs, path, label, scope: "project" }));
      }

      const memDir = join(
        agents.roots("claude").projectsRoot,
        agents.encodeSlug(cwd),
        "memory"
      );
      const present = yield* fs
        .exists(memDir)
        .pipe(Effect.orElseSucceed(() => false));
      if (present) {
        const [bytes, count] = yield* sumMemoryDir({ fs, dir: memDir });
        if (count > 0) {
          files.push({
            label: `memory/ (${count} file${count === 1 ? "" : "s"})`,
            path: memDir,
            bytes,
            tokensEst: Math.round(bytes / BYTES_PER_TOKEN),
            scope: "memory",
          });
        }
      }
    }

    return files;
  }).pipe(Effect.withSpan("Sessions.gatherContextFiles"));
};
