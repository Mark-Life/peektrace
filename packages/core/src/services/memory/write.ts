/** Mutation builders for the memory service: create / update / delete.
 *
 * Every mutation is gated on `CapabilityRegistry.supports({ "memory.crud", agent })`
 * (Claude only) and routes file writes through `WriteFs.atomicWrite` (temp+rename,
 * CAS, path-guard). Deletes use the platform FileSystem `remove` behind an explicit
 * containment check against the agent roots (+ the temp dir).
 */
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import type { AgentId, AgentRegistryShape } from "../agents";
import type { CapabilityRegistryShape } from "../capabilities";
import type { ReadFsShape, WriteFsShape } from "../fs";
import {
  CapabilityUnsupportedError,
  MemoryNotFoundError,
  MemoryValidationError,
} from "./errors";
import {
  composeFile,
  parseFrontmatter,
  serializeFrontmatterBlock,
  type WritableFrontmatter,
} from "./frontmatter";
import {
  indexLineFor,
  insertIndexLine,
  removeIndexLine,
  updateIndexLine,
} from "./index-edit";
import { buildEntry } from "./parse";
import type {
  DanglingRef,
  DeleteResult,
  Frontmatter,
  MemoryVault,
} from "./types";
import { VALID_TYPES } from "./types";

const CAP_ID = "memory.crud";
const INDEX_FILE = "MEMORY.md";
const KEBAB = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** Dependencies for the mutation builders. */
export interface WriteDeps {
  readonly agents: AgentRegistryShape;
  readonly buildVault: (args: {
    readonly slug: string;
    readonly label: string;
  }) => Effect.Effect<MemoryVault>;
  readonly caps: CapabilityRegistryShape;
  readonly fs: FileSystem.FileSystem;
  readonly read: ReadFsShape;
  readonly write: WriteFsShape;
}

/** A subset of editable frontmatter fields. */
interface FrontmatterPatch {
  readonly description?: string;
  readonly name?: string;
  readonly type?: string;
}

/** True when `path` resolves inside one of `roots`. */
const isWithin = (path: string, roots: readonly string[]): boolean => {
  const r = resolve(path);
  return roots.some((root) => {
    const b = resolve(root);
    return r === b || r.startsWith(b + sep);
  });
};

/** Apply an editable-field patch onto a clone of the current frontmatter. */
const applyPatch = (
  current: Frontmatter,
  patch?: FrontmatterPatch
): WritableFrontmatter => {
  const fm: WritableFrontmatter = { ...current, extra: { ...current.extra } };
  if (!patch) {
    return fm;
  }
  if (patch.name !== undefined) {
    fm.name = patch.name;
  }
  if (patch.description !== undefined) {
    fm.description = patch.description;
  }
  if (patch.type !== undefined) {
    fm.type = patch.type;
  }
  fm.raw = serializeFrontmatterBlock(fm);
  return fm;
};

/** Make the create/update/delete methods over the supplied dependencies. */
export const makeWrite = (deps: WriteDeps) => {
  const { agents, read, write, fs, caps, buildVault } = deps;
  const containment = [...agents.allowedRoots, tmpdir()];

  const ensureCapable = (agent: AgentId) =>
    caps.supports({ capabilityId: CAP_ID, agentId: agent }).pipe(
      Effect.flatMap((ok) =>
        ok
          ? Effect.void
          : Effect.fail(
              new CapabilityUnsupportedError({
                capabilityId: CAP_ID,
                agentId: agent,
              })
            )
      )
    );

  const memoryDirFor = (slug: string) =>
    agents.memoryDir({ agent: "claude", slug });

  /** Rewrite MEMORY.md, preserving CAS against its current mtime when it exists. */
  const writeIndex = (args: {
    readonly indexPath: string;
    readonly transform: (raw: string) => string;
    readonly fallback: string;
  }) =>
    fs.exists(args.indexPath).pipe(
      Effect.flatMap((present) =>
        present
          ? Effect.all([
              read.readText(args.indexPath),
              read.stat(args.indexPath),
            ]).pipe(
              Effect.flatMap(([raw, stat]) =>
                write.atomicWrite({
                  path: args.indexPath,
                  content: args.transform(raw),
                  expectedMtime: stat.mtimeMs,
                })
              )
            )
          : write.atomicWrite({
              path: args.indexPath,
              content: args.fallback,
            })
      )
    );

  const statEntry = (args: {
    readonly path: string;
    readonly content: string;
    readonly inIndex: boolean;
  }) =>
    read.stat(args.path).pipe(
      Effect.map((stat) =>
        buildEntry({
          path: args.path,
          text: args.content,
          mtimeMs: stat.mtimeMs,
          inIndex: args.inIndex,
        })
      )
    );

  const readExisting = (args: {
    readonly path: string;
    readonly project: string;
    readonly name: string;
  }) =>
    read
      .readText(args.path)
      .pipe(
        Effect.catchTag("SystemError", () =>
          Effect.fail(
            new MemoryNotFoundError({ project: args.project, name: args.name })
          )
        )
      );

  /** Keep the MEMORY.md pointer line for `name` in sync with `fm`. */
  const syncIndexLine = (mArgs: {
    readonly memoryDir: string;
    readonly name: string;
    readonly fm: WritableFrontmatter;
  }) => {
    const line = indexLineFor({
      label: mArgs.fm.name ?? mArgs.name,
      fileName: `${mArgs.name}.md`,
      ...(mArgs.fm.description === undefined
        ? {}
        : { hook: mArgs.fm.description }),
    });
    return writeIndex({
      indexPath: join(mArgs.memoryDir, INDEX_FILE),
      transform: (raw) => updateIndexLine(raw, mArgs.name, line),
      fallback: `${line}\n`,
    });
  };

  const create = (args: {
    readonly project: string;
    readonly name: string;
    readonly description: string;
    readonly type: string;
    readonly body: string;
    readonly agent?: AgentId;
  }) =>
    Effect.gen(function* () {
      const agent = args.agent ?? "claude";
      yield* ensureCapable(agent);
      if (!KEBAB.test(args.name)) {
        return yield* Effect.fail(
          new MemoryValidationError({
            reason: "name must be kebab-case",
            name: args.name,
          })
        );
      }
      if (!VALID_TYPES.has(args.type)) {
        return yield* Effect.fail(
          new MemoryValidationError({
            reason: `type must be one of ${[...VALID_TYPES].join("/")}`,
            name: args.name,
          })
        );
      }
      const memoryDir = yield* memoryDirFor(args.project);
      const filePath = join(memoryDir, `${args.name}.md`);
      const alreadyExists = yield* fs
        .exists(filePath)
        .pipe(Effect.orElseSucceed(() => false));
      if (alreadyExists) {
        return yield* Effect.fail(
          new MemoryValidationError({
            reason: "a memory with this name already exists",
            name: args.name,
          })
        );
      }

      const fm: WritableFrontmatter = {
        raw: "",
        shape: "flat",
        name: args.name,
        description: args.description,
        type: args.type,
        extra: {},
        hadTrailingMetadataWs: false,
        descriptionQuoted: false,
      };
      fm.raw = serializeFrontmatterBlock(fm);
      const content = composeFile({ frontmatter: fm, body: args.body });
      yield* write.atomicWrite({ path: filePath, content });

      const line = indexLineFor({
        label: args.name,
        fileName: `${args.name}.md`,
        hook: args.description,
      });
      yield* writeIndex({
        indexPath: join(memoryDir, INDEX_FILE),
        transform: (raw) => insertIndexLine(raw, line),
        fallback: `${line}\n`,
      });

      return yield* statEntry({ path: filePath, content, inIndex: true });
    }).pipe(
      Effect.withSpan("Memory.create", { attributes: { name: args.name } })
    );

  const update = (args: {
    readonly project: string;
    readonly name: string;
    readonly frontmatter?: {
      readonly name?: string;
      readonly description?: string;
      readonly type?: string;
    };
    readonly body?: string;
    readonly expectedMtime?: number;
    readonly agent?: AgentId;
  }) =>
    Effect.gen(function* () {
      const agent = args.agent ?? "claude";
      yield* ensureCapable(agent);
      const patch = args.frontmatter;
      if (patch?.type !== undefined && !VALID_TYPES.has(patch.type)) {
        return yield* Effect.fail(
          new MemoryValidationError({
            reason: `type must be one of ${[...VALID_TYPES].join("/")}`,
            name: args.name,
          })
        );
      }

      const memoryDir = yield* memoryDirFor(args.project);
      const filePath = join(memoryDir, `${args.name}.md`);
      const text = yield* readExisting({
        path: filePath,
        project: args.project,
        name: args.name,
      });
      const { frontmatter: current, body: currentBody } =
        parseFrontmatter(text);
      const fm = applyPatch(current, patch);
      const content = composeFile({
        frontmatter: fm,
        body: args.body ?? currentBody,
      });

      yield* write.atomicWrite({
        path: filePath,
        content,
        ...(args.expectedMtime === undefined
          ? {}
          : { expectedMtime: args.expectedMtime }),
      });

      if (patch?.name !== undefined || patch?.description !== undefined) {
        yield* syncIndexLine({ memoryDir, name: args.name, fm });
      }

      return yield* statEntry({ path: filePath, content, inIndex: true });
    }).pipe(
      Effect.withSpan("Memory.update", { attributes: { name: args.name } })
    );

  const remove = (args: {
    readonly project: string;
    readonly name: string;
    readonly agent?: AgentId;
  }) =>
    Effect.gen(function* () {
      const agent = args.agent ?? "claude";
      yield* ensureCapable(agent);
      const memoryDir = yield* memoryDirFor(args.project);
      const filePath = join(memoryDir, `${args.name}.md`);

      const vault = yield* buildVault({
        slug: args.project,
        label: args.project,
      });
      const exists = vault.entries.some((e) => e.slug === args.name);
      if (!exists) {
        return yield* Effect.fail(
          new MemoryNotFoundError({ project: args.project, name: args.name })
        );
      }

      const dangling: DanglingRef[] = vault.graph.edges
        .filter(
          (e) =>
            (e.kind === "wiki" || e.kind === "markdown") &&
            e.resolvedTo === args.name &&
            e.from !== args.name
        )
        .map((e) => ({
          from: e.from,
          target: args.name,
          ...(e.line === undefined ? {} : { line: e.line }),
        }));

      if (!isWithin(filePath, containment)) {
        return yield* Effect.fail(
          new MemoryValidationError({
            reason: "refusing to delete outside the agent roots",
            name: args.name,
          })
        );
      }
      yield* fs.remove(filePath);

      yield* writeIndex({
        indexPath: join(memoryDir, INDEX_FILE),
        transform: (raw) => removeIndexLine(raw, args.name).content,
        fallback: "",
      });

      return { slug: args.name, dangling } satisfies DeleteResult;
    }).pipe(
      Effect.withSpan("Memory.delete", { attributes: { name: args.name } })
    );

  return { create, update, delete: remove };
};
