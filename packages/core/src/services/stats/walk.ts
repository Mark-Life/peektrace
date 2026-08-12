/**
 * The corpus walk: find every transcript, classify it, and stream it through
 * one fold without holding the corpus in memory.
 *
 * Discovery goes per agent through `AgentRegistry.listSessionFiles`, so Codex,
 * Pi and OpenCode come free. Claude's listing is non-recursive, and its nested
 * transcripts are the trap: everything under a session's `subagents` dir, and
 * the `agent-*.jsonl` files under `subagents/workflows/wf_<id>`, hold roughly a
 * tenth of a real corpus and a one-level glob never sees them. `journal.jsonl`
 * is skipped explicitly, not by a regex that happens to miss it.
 *
 * Order is the sorted absolute path, never directory read order. Parsing runs
 * with bounded concurrency but the fold sees files in path order, because
 * `Stream.mapEffect` is ordered unless asked otherwise — so a float sum cannot
 * drift between two runs.
 */
import type { FileSystem } from "@effect/platform";
import { Effect, Option, Stream } from "effect";
import type { AgentId } from "../agent-id";
import {
  type AgentRegistryShape,
  type SessionFileRef,
  type SessionLayout,
  sourcesOf,
} from "../agents";
import type { FileStat } from "../fs";
import { compareByPath, sortBy } from "./rank";
import type { TranscriptKind } from "./schema";

/** Files parsed at once. The sessions service uses 8 for header-only work;
 * a full parse holds a whole session, so this stays lower. */
export const WALK_CONCURRENCY = 4;

/** Never a transcript: the harness's own append-only log. */
const JOURNAL = "journal.jsonl";

const JSONL = /\.jsonl$/;

/** Layouts that nest subagent transcripts under a session dir. */
const NESTED_LAYOUTS: ReadonlySet<SessionLayout> = new Set(["claude-projects"]);

/** One transcript located on disk, classified by where it sits in the tree. */
export interface TranscriptRef extends SessionFileRef {
  readonly agent: AgentId;
  readonly kind: TranscriptKind;
  /** Parent session id for a nested transcript, else null. */
  readonly parent: string | null;
  /** Workflow run id (the `wf_*` dir) for a workflow agent, else null. */
  readonly run: string | null;
}

/** Where a transcript sits, from its path parts relative to the project dir. */
export const classifyParts = (
  parts: readonly string[]
): Pick<TranscriptRef, "kind" | "parent" | "run"> => {
  if (parts.length <= 1) {
    return { kind: "main", parent: null, run: null };
  }
  const parent = parts[0] ?? null;
  const wfIndex = parts.findIndex((p) => p.startsWith("wf_"));
  if (wfIndex > 0 && parts[wfIndex - 1] === "workflows") {
    return { kind: "workflow-agent", parent, run: parts[wfIndex] ?? null };
  }
  return { kind: "subagent", parent, run: null };
};

/** Directory entries of `dir`, split into dirs and files. Never fails. */
const readEntries = (fs: FileSystem.FileSystem, dir: string) =>
  fs.readDirectory(dir).pipe(
    Effect.orElseSucceed(() => [] as readonly string[]),
    Effect.flatMap((names) =>
      Effect.forEach(names, (name) =>
        fs.stat(`${dir}/${name}`).pipe(
          Effect.map((info) => ({ name, type: info.type })),
          Effect.orElseSucceed(() => ({ name, type: "Other" as const }))
        )
      )
    )
  );

/**
 * Every nested transcript under one project dir. Files sitting directly in the
 * project dir are the mains the registry already listed, so they are skipped.
 */
const nestedIn = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly projectDir: string;
}): Effect.Effect<readonly { path: string; parts: string[] }[]> => {
  const { fs, projectDir } = args;
  const walk = (
    dir: string,
    parts: readonly string[]
  ): Effect.Effect<readonly { path: string; parts: string[] }[]> =>
    readEntries(fs, dir).pipe(
      Effect.flatMap((entries) =>
        Effect.forEach(entries, (entry) => {
          const path = `${dir}/${entry.name}`;
          if (entry.type === "Directory") {
            return walk(path, [...parts, entry.name]);
          }
          const keep =
            entry.type === "File" &&
            entry.name !== JOURNAL &&
            JSONL.test(entry.name) &&
            parts.length > 0;
          return Effect.succeed(
            keep ? [{ path, parts: [...parts, entry.name] }] : []
          );
        })
      ),
      Effect.map((groups) => groups.flat())
    );
  return walk(projectDir, []);
};

/** Nested transcripts across every source root of a nesting layout. */
const nestedRefs = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly agent: AgentId;
}): Effect.Effect<readonly TranscriptRef[]> => {
  const { fs, agents, agent } = args;
  const roots = agents.roots(agent);
  if (!NESTED_LAYOUTS.has(roots.layout)) {
    return Effect.succeed([]);
  }
  return Effect.forEach(sourcesOf(roots), (source) =>
    agents.listProjectSlugs(agent).pipe(
      Effect.orElseSucceed(() => [] as readonly string[]),
      Effect.flatMap((slugs) =>
        Effect.forEach(slugs, (slug) =>
          nestedIn({ fs, projectDir: `${source.projectsRoot}/${slug}` }).pipe(
            Effect.map((found) =>
              found.map(
                ({ path, parts }) =>
                  ({
                    agent,
                    path,
                    slug,
                    id: (parts.at(-1) ?? "").replace(JSONL, ""),
                    ...classifyParts(parts),
                    ...(source.isDefault
                      ? {}
                      : { source: { id: source.id, label: source.label } }),
                  }) satisfies TranscriptRef
              )
            )
          )
        )
      ),
      Effect.map((groups) => groups.flat())
    )
  ).pipe(
    Effect.map((groups) => groups.flat()),
    Effect.orElseSucceed(() => [] as readonly TranscriptRef[])
  );
};

/**
 * Every transcript for one agent: the registry's mains plus the nested
 * subagent and workflow-agent transcripts, deduped by path and path-sorted.
 */
export const listTranscripts = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly agent: AgentId;
}): Effect.Effect<readonly TranscriptRef[]> => {
  const { fs, agents, agent } = args;
  return Effect.all([
    agents.listSessionFiles(agent),
    nestedRefs({ fs, agents, agent }),
  ]).pipe(
    Effect.map(([mains, nested]) => {
      const seen = new Set<string>();
      const out: TranscriptRef[] = [];
      const mainRefs = mains
        .filter((ref) => !ref.path.endsWith(JOURNAL))
        .map(
          (ref) =>
            ({
              ...ref,
              agent,
              kind: "main",
              parent: null,
              run: null,
            }) satisfies TranscriptRef
        );
      for (const ref of [...mainRefs, ...nested]) {
        if (!seen.has(ref.path)) {
          seen.add(ref.path);
          out.push(ref);
        }
      }
      return sortBy(out, compareByPath) as readonly TranscriptRef[];
    }),
    Effect.withSpan("Stats.listTranscripts", { attributes: { agent } })
  );
};

/** Every transcript across the requested agents, path-sorted as one corpus. */
export const listCorpus = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly scope: readonly AgentId[];
}): Effect.Effect<readonly TranscriptRef[]> => {
  const { fs, agents, scope } = args;
  return Effect.forEach(scope, (agent) =>
    listTranscripts({ fs, agents, agent })
  ).pipe(
    Effect.map(
      (groups) =>
        sortBy(groups.flat(), compareByPath) as readonly TranscriptRef[]
    ),
    Effect.withSpan("Stats.listCorpus")
  );
};

/**
 * `mtime` + `size` of a transcript, the cache key inputs. Both 0 when the path
 * is not a real file (OpenCode refs are `<dataDir>#<id>` handles), which the
 * cache reads as "never cacheable" rather than "unchanged".
 */
export const statOf = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly ref: TranscriptRef;
}): Effect.Effect<FileStat> => {
  const { fs, ref } = args;
  return fs.stat(ref.path).pipe(
    Effect.map((info) => ({
      size: Number(info.size),
      mtimeMs: Option.match(info.mtime, {
        onNone: () => 0,
        onSome: (d) => d.getTime(),
      }),
    })),
    Effect.orElseSucceed(() => ({ size: 0, mtimeMs: 0 }) satisfies FileStat)
  );
};

/**
 * Stream the corpus through `perFile`, folding each file's result and dropping
 * it before the next. Peak memory is `concurrency` files plus the accumulators,
 * which are bounded by distinct keys, not by row count.
 */
export const foldCorpus = <A, E, R>(args: {
  readonly refs: readonly TranscriptRef[];
  readonly concurrency?: number;
  readonly perFile: (ref: TranscriptRef) => Effect.Effect<A, E, R>;
  readonly onFile: (result: A, ref: TranscriptRef) => void;
}): Effect.Effect<void, E, R> => {
  const { refs, concurrency = WALK_CONCURRENCY, perFile, onFile } = args;
  return Stream.fromIterable(refs).pipe(
    Stream.mapEffect(
      (ref) => perFile(ref).pipe(Effect.map((result) => ({ ref, result }))),
      { concurrency }
    ),
    Stream.runForEach(({ ref, result }) =>
      Effect.sync(() => {
        onFile(result, ref);
      })
    ),
    Effect.withSpan("Stats.foldCorpus", {
      attributes: { transcripts: refs.length },
    })
  );
};
