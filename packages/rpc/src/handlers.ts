/** Handlers for the Peektrace RPC group + the layer that wires them to core.
 *
 * Handlers are thin: each calls one core service method and maps any core
 * `Data.TaggedError` to its `Schema.TaggedError` wire twin (see `contract.ts`).
 * Infrastructure failures that are not part of the domain contract
 * (`PlatformError`, `AgentUnsupportedError`) are converted to defects — they are
 * not meant to round-trip as typed RPC failures.
 *
 * `makeHandlersLayer` composes the core services once so both the CLI `serve`
 * and the in-process test share identical wiring. The platform `FileSystem` and
 * the `AgentRegistry` are injectable so tests can point Claude resolution at the
 * committed fixtures.
 */
import type { FileSystem } from "@effect/platform";
import type { PlatformError } from "@effect/platform/Error";
import { BunFileSystem } from "@effect/platform-bun";
import {
  type AgentRegistry,
  AgentRegistryLive,
  type AgentUnsupportedError,
  CapabilityRegistry,
  CapabilityRegistryLive,
  type CapabilityUnsupportedError as CoreCapabilityUnsupportedError,
  type FileChangedError as CoreFileChangedError,
  type MemoryNotFoundError as CoreMemoryNotFoundError,
  type MemoryValidationError as CoreMemoryValidationError,
  type PathOutsideRootError as CorePathOutsideRootError,
  type SessionNotFoundError as CoreSessionNotFoundError,
  type TranscriptParseError as CoreTranscriptParseError,
  FsLive,
  MemoryService,
  MemoryServiceLive,
  SessionsService,
  SessionsServiceLive,
  WatchService,
  WatchServiceLive,
} from "@workspace/core";
import { Effect, Layer } from "effect";
import {
  CapabilityUnsupportedError,
  FileChangedError,
  type MemoryFrontmatterPatch,
  MemoryNotFoundError,
  MemoryValidationError,
  PathOutsideRootError,
  PeektraceRpcs,
  SessionNotFoundError,
  TranscriptParseError,
  type WireError,
} from "./contract";

/** Every core error a fallible handler can encounter. */
type CoreError =
  | CoreCapabilityUnsupportedError
  | CoreMemoryValidationError
  | CoreMemoryNotFoundError
  | CoreFileChangedError
  | CorePathOutsideRootError
  | CoreSessionNotFoundError
  | CoreTranscriptParseError
  | AgentUnsupportedError
  | PlatformError;

/** Map a core domain error to its wire twin; defect on infrastructure errors. */
const toWire = (error: CoreError): Effect.Effect<never, WireError> => {
  switch (error._tag) {
    case "CapabilityUnsupportedError":
      return Effect.fail(
        new CapabilityUnsupportedError({
          capabilityId: error.capabilityId,
          agentId: error.agentId,
        })
      );
    case "MemoryValidationError":
      return Effect.fail(
        new MemoryValidationError({
          reason: error.reason,
          ...(error.name === undefined ? {} : { name: error.name }),
        })
      );
    case "MemoryNotFoundError":
      return Effect.fail(
        new MemoryNotFoundError({ project: error.project, name: error.name })
      );
    case "FileChangedError":
      return Effect.fail(
        new FileChangedError({ path: error.path, reason: error.reason })
      );
    case "PathOutsideRootError":
      return Effect.fail(
        new PathOutsideRootError({ path: error.path, roots: error.roots })
      );
    case "SessionNotFoundError":
      return Effect.fail(
        new SessionNotFoundError({
          id: error.id,
          searchedRoot: error.searchedRoot,
        })
      );
    case "TranscriptParseError":
      return Effect.fail(
        new TranscriptParseError({ path: error.path, reason: error.reason })
      );
    default:
      return Effect.die(error);
  }
};

/** Surface core domain errors as typed wire failures; defect on the rest. */
const wire = <A, R>(effect: Effect.Effect<A, CoreError, R>) =>
  Effect.catchAll(effect, toWire);

/** Build the memory-update frontmatter patch, dropping undefined fields. */
const buildMemoryPatch = (
  frontmatter: typeof MemoryFrontmatterPatch.Type | undefined
) => {
  if (frontmatter === undefined) {
    return;
  }
  return {
    ...(frontmatter.name === undefined ? {} : { name: frontmatter.name }),
    ...(frontmatter.description === undefined
      ? {}
      : { description: frontmatter.description }),
    ...(frontmatter.type === undefined ? {} : { type: frontmatter.type }),
  };
};

/**
 * Wrap every handler in a root span so a tracer can assemble one wide event per
 * RPC call. rpc gains no telemetry dependency — this is a plain string attribute.
 */
const withRootSpans = <H extends Record<string, (...args: never[]) => unknown>>(
  handlers: H,
  rootSpans: boolean
): H => {
  if (!rootSpans) {
    return handlers;
  }
  return Object.fromEntries(
    Object.entries(handlers).map(([tag, handler]) => [
      tag,
      (...args: never[]) =>
        (handler(...args) as Effect.Effect<unknown, unknown, unknown>).pipe(
          Effect.withSpan(`rpc.${tag}`, {
            attributes: { "peektrace.root": true, "peektrace.kind": "rpc" },
          })
        ),
    ])
  ) as H;
};

/**
 * Refuse a mutating RPC under read-only mode with a typed wire failure the UI
 * renders cleanly (no write is attempted; the disk is never touched).
 */
const readOnlyRefusal = () =>
  Effect.fail(
    new MemoryValidationError({
      reason: "read-only mode is enabled; refusing to write",
    })
  );

/** Handler layer factory: requires the core services in context. */
const makeHandlersLive = (rootSpans: boolean, readOnly: boolean) =>
  PeektraceRpcs.toLayer(
    Effect.gen(function* () {
      const sessions = yield* SessionsService;
      const memory = yield* MemoryService;
      const caps = yield* CapabilityRegistry;
      const watch = yield* WatchService;

      return withRootSpans(
        {
          "capabilities.list": () => caps.list(),
          "watch.poll": () => watch.versions,
          "sessions.list": ({ project, agent }) =>
            sessions
              .list()
              .pipe(
                Effect.map((headers) =>
                  headers.filter(
                    (header) =>
                      (!project || header.project === project) &&
                      (!agent || header.agent === agent)
                  )
                )
              ),
          "sessions.get": ({ id, redact }) =>
            wire(
              sessions.parse({
                id,
                ...(redact === undefined ? {} : { redact }),
              })
            ),
          "sessions.analyze": ({ id, window, dumbZone, redact }) =>
            wire(
              sessions.analyze({
                id,
                ...(window === undefined ? {} : { window }),
                ...(dumbZone === undefined ? {} : { dumbZone }),
                ...(redact === undefined ? {} : { redact }),
              })
            ),
          "memory.allVaults": () => memory.getAllVaults(),
          "memory.projects": () => memory.listProjects(),
          "memory.vault": ({ project }) => memory.getVault(project),
          "memory.create": ({ project, name, description, type, body }) =>
            readOnly
              ? readOnlyRefusal()
              : wire(memory.create({ project, name, description, type, body })),
          "memory.update": ({
            project,
            name,
            frontmatter,
            body,
            expectedMtime,
          }) => {
            if (readOnly) {
              return readOnlyRefusal();
            }
            const patch = buildMemoryPatch(frontmatter);
            return wire(
              memory.update({
                project,
                name,
                ...(patch === undefined ? {} : { frontmatter: patch }),
                ...(body === undefined ? {} : { body }),
                ...(expectedMtime === undefined ? {} : { expectedMtime }),
              })
            );
          },
          "memory.delete": ({ project, name }) =>
            readOnly
              ? readOnlyRefusal()
              : wire(memory.delete({ project, name })),
        },
        rootSpans
      );
    })
  );

/** Injection points for the core service wiring. */
export interface HandlersLayerOptions {
  /** Provide a self-contained `AgentRegistry` (e.g. pointed at test fixtures). */
  readonly agents?: Layer.Layer<AgentRegistry>;
  /** Provide a custom platform `FileSystem` (defaults to Bun). */
  readonly fileSystem?: Layer.Layer<FileSystem.FileSystem>;
  /** Refuse every mutating RPC with a typed failure (safe mode); no disk writes. */
  readonly readOnly?: boolean;
  /** Wrap each handler in a root span so a tracer emits one wide event per call. */
  readonly rootSpans?: boolean;
}

/** Compose the core services (sessions + memory + capabilities) over the FS. */
const coreServicesLayer = (options?: HandlersLayerOptions) => {
  const fileSystem = options?.fileSystem ?? BunFileSystem.layer;
  const agents =
    options?.agents ?? AgentRegistryLive.pipe(Layer.provide(fileSystem));
  const fs = FsLive.pipe(Layer.provide(agents), Layer.provide(fileSystem));
  const caps = CapabilityRegistryLive;
  const sessions = SessionsServiceLive.pipe(
    Layer.provide(agents),
    Layer.provide(fileSystem)
  );
  const memory = MemoryServiceLive.pipe(
    Layer.provide(agents),
    Layer.provide(fs),
    Layer.provide(caps),
    Layer.provide(fileSystem)
  );
  const watch = WatchServiceLive.pipe(
    Layer.provide(agents),
    Layer.provide(fileSystem)
  );
  return Layer.mergeAll(sessions, memory, caps, watch);
};

/**
 * The fully-wired handler layer, ready to mount on an `RpcServer` or to drive an
 * in-process client. Pass `agents`/`fileSystem` to retarget IO in tests.
 */
export const makeHandlersLayer = (options?: HandlersLayerOptions) =>
  makeHandlersLive(
    options?.rootSpans ?? false,
    options?.readOnly ?? false
  ).pipe(Layer.provide(coreServicesLayer(options)));
