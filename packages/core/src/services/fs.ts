import { createHash, randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve, sep } from "node:path";
import { FileSystem } from "@effect/platform";
import type { PlatformError } from "@effect/platform/Error";
import { Context, Data, Effect, Layer, Option } from "effect";
import { AgentRegistry } from "./agents";

const TEMP_TOKEN_BYTES = 8;

/** Normalized stat used for compare-and-swap. */
export interface FileStat {
  readonly mtimeMs: number;
  readonly size: number;
}

/** Raised when on-disk mtime/hash no longer matches the caller's expectation. */
export class FileChangedError extends Data.TaggedError("FileChangedError")<{
  readonly path: string;
  readonly reason: "mtime" | "hash" | "missing";
}> {}

/** Raised when a write cannot proceed (e.g. lock already held). */
export class WriteDeniedError extends Data.TaggedError("WriteDeniedError")<{
  readonly path: string;
  readonly reason: string;
}> {}

/** Raised when a write target escapes every declared agent root (and the temp dir). */
export class PathOutsideRootError extends Data.TaggedError(
  "PathOutsideRootError"
)<{
  readonly path: string;
  readonly roots: readonly string[];
}> {}

/** Read-only filesystem surface. Present in both `FsLive` and `FsReadOnly`. */
export interface ReadFsShape {
  readonly readText: (path: string) => Effect.Effect<string, PlatformError>;
  readonly stat: (path: string) => Effect.Effect<FileStat, PlatformError>;
}

/** Compare-and-swap expectation for an atomic write. */
export interface WriteExpectation {
  readonly expectedHash?: string;
  readonly expectedMtime?: number;
}

/** Mutating filesystem surface. Present ONLY in `FsLive`. */
export interface WriteFsShape {
  readonly atomicWrite: (
    args: {
      readonly path: string;
      readonly content: string;
    } & WriteExpectation
  ) => Effect.Effect<
    void,
    PlatformError | FileChangedError | PathOutsideRootError
  >;
  readonly withFileLock: <A, E, R>(
    path: string,
    effect: Effect.Effect<A, E, R>
  ) => Effect.Effect<A, E | WriteDeniedError | PathOutsideRootError, R>;
}

/** Read tag — provided by every FS layer. */
export class ReadFs extends Context.Tag("@peephole/ReadFs")<
  ReadFs,
  ReadFsShape
>() {}

/** Write tag — provided only by the read-write layer. */
export class WriteFs extends Context.Tag("@peephole/WriteFs")<
  WriteFs,
  WriteFsShape
>() {}

/** sha256 hex of a string. */
const sha256 = (content: string): string =>
  createHash("sha256").update(content).digest("hex");

/** True when `path` resolves inside one of `roots`. */
const isWithin = (path: string, roots: readonly string[]): boolean => {
  const resolved = resolve(path);
  return roots.some((root) => {
    const base = resolve(root);
    return resolved === base || resolved.startsWith(base + sep);
  });
};

/** Normalize platform stat into the CAS-friendly shape. */
const toFileStat = (info: FileSystem.File.Info): FileStat => ({
  size: Number(info.size),
  mtimeMs: Option.match(info.mtime, {
    onNone: () => 0,
    onSome: (date) => date.getTime(),
  }),
});

/** Build the read surface from a platform FileSystem. */
const makeRead = (fs: FileSystem.FileSystem): ReadFsShape => ({
  readText: (path) =>
    fs
      .readFileString(path)
      .pipe(Effect.withSpan("Fs.readText", { attributes: { path } })),
  stat: (path) =>
    fs
      .stat(path)
      .pipe(
        Effect.map(toFileStat),
        Effect.withSpan("Fs.stat", { attributes: { path } })
      ),
});

/** Verify a CAS expectation against the current on-disk file. */
const checkExpectation = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly path: string;
  readonly expectation: WriteExpectation;
}) => {
  const { fs, path, expectation } = args;
  if (
    expectation.expectedMtime === undefined &&
    expectation.expectedHash === undefined
  ) {
    return Effect.void;
  }
  return fs.exists(path).pipe(
    Effect.flatMap((present) =>
      present
        ? Effect.void
        : Effect.fail(new FileChangedError({ path, reason: "missing" }))
    ),
    Effect.flatMap(() =>
      expectation.expectedMtime === undefined
        ? Effect.void
        : fs
            .stat(path)
            .pipe(
              Effect.flatMap((info) =>
                toFileStat(info).mtimeMs === expectation.expectedMtime
                  ? Effect.void
                  : Effect.fail(new FileChangedError({ path, reason: "mtime" }))
              )
            )
    ),
    Effect.flatMap(() =>
      expectation.expectedHash === undefined
        ? Effect.void
        : fs
            .readFileString(path)
            .pipe(
              Effect.flatMap((current) =>
                sha256(current) === expectation.expectedHash
                  ? Effect.void
                  : Effect.fail(new FileChangedError({ path, reason: "hash" }))
              )
            )
    )
  );
};

/** Build the write surface from a platform FileSystem + allowed roots. */
const makeWrite = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly allowedRoots: readonly string[];
}): WriteFsShape => {
  const { fs, allowedRoots } = args;

  const guardPath = (
    path: string
  ): Effect.Effect<void, PathOutsideRootError> =>
    isWithin(path, allowedRoots)
      ? Effect.void
      : Effect.fail(new PathOutsideRootError({ path, roots: allowedRoots }));

  const atomicWrite: WriteFsShape["atomicWrite"] = ({
    path,
    content,
    ...expectation
  }) =>
    guardPath(path).pipe(
      Effect.flatMap(() => checkExpectation({ fs, path, expectation })),
      Effect.flatMap(() => {
        const token = randomBytes(TEMP_TOKEN_BYTES).toString("hex");
        const tempPath = join(dirname(path), `.${basename(path)}.tmp-${token}`);
        return fs
          .writeFileString(tempPath, content)
          .pipe(Effect.flatMap(() => fs.rename(tempPath, path)));
      }),
      Effect.withSpan("Fs.atomicWrite", { attributes: { path } })
    );

  const withFileLock: WriteFsShape["withFileLock"] = (path, effect) => {
    const lockPath = `${path}.lock`;
    return guardPath(path).pipe(
      Effect.flatMap(() =>
        Effect.acquireUseRelease(
          fs
            .makeDirectory(lockPath)
            .pipe(
              Effect.mapError(
                () => new WriteDeniedError({ path, reason: "lock held" })
              )
            ),
          () => effect,
          () => fs.remove(lockPath, { recursive: true }).pipe(Effect.ignore)
        )
      ),
      Effect.withSpan("Fs.withFileLock", { attributes: { path } })
    );
  };

  return { atomicWrite, withFileLock };
};

/** Read-only layer: provides `ReadFs` only. Mutations are absent at the type level. */
export const FsReadOnly = Layer.effect(
  ReadFs,
  Effect.map(FileSystem.FileSystem, makeRead)
);

const writeLayer = Layer.effect(
  WriteFs,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const agents = yield* AgentRegistry;
    return makeWrite({ fs, allowedRoots: [...agents.allowedRoots, tmpdir()] });
  })
);

/** Read-write layer: provides both `ReadFs` and `WriteFs`. */
export const FsLive = Layer.merge(FsReadOnly, writeLayer);
