/** Read-model builders for the memory service: project discovery, single-vault
 * parse (entries + index + budget + diff + graph + type counts), and the
 * cross-project default. All IO flows through `ReadFs` + the platform FileSystem.
 */
import { join } from "node:path";
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import type { AgentRegistryShape } from "../agents";
import type { ReadFsShape } from "../fs";
import { buildGraph } from "./graph";
import {
  buildEntry,
  indexBudget,
  normalizeSlug,
  parseIndexContent,
} from "./parse";
import { labelSlugs } from "./resolve";
import type {
  AllVaults,
  MemoryEntry,
  MemoryIndex,
  MemoryVault,
  ProjectSummary,
  VaultDiff,
  VaultState,
} from "./types";

const INDEX_FILE = "MEMORY.md";

/** Dependencies for the read builders. */
export interface ReadDeps {
  readonly agents: AgentRegistryShape;
  readonly fs: FileSystem.FileSystem;
  readonly read: ReadFsShape;
}

/** Classify the overall vault state from its parts. */
const classifyState = (args: {
  readonly exists: boolean;
  readonly entries: readonly MemoryEntry[];
  readonly index: MemoryIndex | null;
}): VaultState => {
  if (!args.exists) {
    return "absent";
  }
  if (args.index?.isMonolithic) {
    return "monolithic";
  }
  if (args.entries.length === 0 && !args.index) {
    return "empty";
  }
  return "ok";
};

/** Build the index<->files diff (orphan files vs dangling index entries). */
const buildDiff = (args: {
  readonly entries: readonly MemoryEntry[];
  readonly index: MemoryIndex | null;
}): VaultDiff => {
  const orphans = args.entries.filter((e) => !e.inIndex).map((e) => e.slug);
  const fileSlugs = new Set(args.entries.map((e) => normalizeSlug(e.slug)));
  const dangling = (args.index?.entries ?? [])
    .filter((e) => e.targetSlug && !fileSlugs.has(e.targetSlug))
    .map((e) => ({
      target: e.target ?? e.targetSlug ?? "",
      line: e.lineNumber,
    }));
  return { orphans, dangling };
};

/** Make the read-model methods over the supplied dependencies. */
export const makeRead = (deps: ReadDeps) => {
  const { agents, read, fs } = deps;

  const summarizeDir = (memoryDir: string) =>
    fs.readDirectory(memoryDir).pipe(
      Effect.map((names) => ({
        fileCount: names.filter((n) => n.endsWith(".md") && n !== INDEX_FILE)
          .length,
        hasIndex: names.includes(INDEX_FILE),
      })),
      Effect.orElseSucceed(() => ({ fileCount: 0, hasIndex: false }))
    );

  const readIndex = (memoryDir: string, names: readonly string[]) => {
    if (!names.includes(INDEX_FILE)) {
      return Effect.succeed<MemoryIndex | null>(null);
    }
    const path = join(memoryDir, INDEX_FILE);
    return read.readText(path).pipe(
      Effect.map((raw) => parseIndexContent({ raw, path })),
      Effect.orElseSucceed(() => null)
    );
  };

  const readEntry = (args: {
    readonly path: string;
    readonly indexed: ReadonlySet<string>;
  }) =>
    Effect.all([read.readText(args.path), read.stat(args.path)]).pipe(
      Effect.map(([text, stat]) =>
        buildEntry({
          path: args.path,
          text,
          mtimeMs: stat.mtimeMs,
          inIndex: args.indexed.has(normalizeSlug(args.path)),
        })
      ),
      Effect.option
    );

  const classifyNames = (memoryDir: string, names: readonly string[]) =>
    Effect.forEach(names, (name) =>
      fs.stat(join(memoryDir, name)).pipe(
        Effect.map((info) => ({ name, isFile: info.type === "File" })),
        Effect.orElseSucceed(() => ({ name, isFile: false }))
      )
    );

  const buildVault = (args: {
    readonly slug: string;
    readonly label: string;
  }): Effect.Effect<MemoryVault> =>
    Effect.gen(function* () {
      const { slug, label } = args;
      const memoryDir = yield* agents
        .memoryDir({ agent: "claude", slug })
        .pipe(Effect.orElseSucceed(() => join(slug, "memory")));
      const exists = yield* fs
        .exists(memoryDir)
        .pipe(Effect.orElseSucceed(() => false));

      if (!exists) {
        return absentVault({ slug, project: label, memoryDir });
      }

      const names = yield* fs
        .readDirectory(memoryDir)
        .pipe(Effect.orElseSucceed(() => [] as readonly string[]));
      const classified = yield* classifyNames(memoryDir, names);
      const files = classified.filter((c) => c.isFile);
      const mdFiles = files.filter(
        (c) => c.name.endsWith(".md") && c.name !== INDEX_FILE
      );
      const strayFiles = files
        .filter((c) => !c.name.endsWith(".md"))
        .map((c) => c.name)
        .sort((a, b) => a.localeCompare(b));

      const index = yield* readIndex(memoryDir, names);
      const indexed = new Set(
        (index?.entries ?? [])
          .map((e) => e.targetSlug)
          .filter((s): s is string => Boolean(s))
      );

      const parsed = yield* Effect.forEach(
        mdFiles.sort((a, b) => a.name.localeCompare(b.name)),
        (c) => readEntry({ path: join(memoryDir, c.name), indexed })
      );
      const entries = parsed.flatMap((o) =>
        o._tag === "Some" ? [o.value] : []
      );

      const graph = buildGraph({
        entries,
        indexEntries: index?.entries ?? [],
      });
      const diff = buildDiff({ entries, index });
      const typeCounts: Record<string, number> = {};
      for (const e of entries) {
        const t = e.frontmatter.type ?? "unknown";
        typeCounts[t] = (typeCounts[t] ?? 0) + 1;
      }

      return {
        slug,
        project: label,
        memoryDir,
        state: classifyState({ exists, entries, index }),
        index,
        entries,
        strayFiles,
        budget: indexBudget(index),
        graph,
        diff,
        typeCounts,
        totalBytes: entries.reduce((s, e) => s + e.size, 0),
      } satisfies MemoryVault;
    }).pipe(
      Effect.withSpan("Memory.buildVault", { attributes: { slug: args.slug } })
    );

  const listProjects = (): Effect.Effect<readonly ProjectSummary[]> =>
    Effect.gen(function* () {
      const slugs = yield* agents
        .listProjectSlugs("claude")
        .pipe(Effect.orElseSucceed(() => [] as readonly string[]));
      const labels = labelSlugs(slugs);
      const summaries = yield* Effect.forEach(slugs, (slug) =>
        Effect.gen(function* () {
          const memoryDir = yield* agents
            .memoryDir({ agent: "claude", slug })
            .pipe(Effect.orElseSucceed(() => join(slug, "memory")));
          const present = yield* fs
            .exists(memoryDir)
            .pipe(Effect.orElseSucceed(() => false));
          if (!present) {
            return null;
          }
          const { fileCount, hasIndex } = yield* summarizeDir(memoryDir);
          if (fileCount === 0 && !hasIndex) {
            return null;
          }
          return {
            slug,
            project: labels.get(slug) ?? slug,
            memoryDir,
            fileCount,
            hasIndex,
          } satisfies ProjectSummary;
        })
      );
      return summaries
        .filter((s): s is ProjectSummary => s !== null)
        .sort(
          (a, b) => b.fileCount - a.fileCount || a.slug.localeCompare(b.slug)
        );
    }).pipe(Effect.withSpan("Memory.listProjects"));

  const getVault = (slug: string): Effect.Effect<MemoryVault> =>
    Effect.gen(function* () {
      const slugs = yield* agents
        .listProjectSlugs("claude")
        .pipe(Effect.orElseSucceed(() => [slug] as readonly string[]));
      const labels = labelSlugs(
        slugs.includes(slug) ? slugs : [...slugs, slug]
      );
      return yield* buildVault({ slug, label: labels.get(slug) ?? slug });
    });

  const getAllVaults = (): Effect.Effect<AllVaults> =>
    Effect.gen(function* () {
      const projects = yield* listProjects();
      const vaults = yield* Effect.forEach(projects, (p) =>
        buildVault({ slug: p.slug, label: p.project })
      );
      return { projects, vaults };
    }).pipe(Effect.withSpan("Memory.getAllVaults"));

  return { buildVault, listProjects, getVault, getAllVaults };
};

/** A vault for a project whose memory dir is absent on disk. */
const absentVault = (args: {
  readonly slug: string;
  readonly project: string;
  readonly memoryDir: string;
}): MemoryVault => ({
  slug: args.slug,
  project: args.project,
  memoryDir: args.memoryDir,
  state: "absent",
  index: null,
  entries: [],
  strayFiles: [],
  budget: indexBudget(null),
  graph: { nodes: [], edges: [], orphans: [] },
  diff: { orphans: [], dangling: [] },
  typeCounts: {},
  totalBytes: 0,
});
