/** Verify-then-install for `peektrace upgrade`.
 *
 * Two ways to reach the new binary, one way to trust it. A delta upgrade
 * (`./patch`) patches the running binary in place; a full download fetches the
 * release asset, preferring the gzipped copy. Either way the bytes that land on
 * disk are checked against the `SHA256SUMS` entry for the *uncompressed* asset
 * (mirroring `scripts/install.sh`' `verify_checksum`) before they replace
 * anything. A mismatch or a missing manifest entry aborts *without* touching
 * the installed binary.
 */
import { chmod, open, rename, stat, unlink } from "node:fs/promises";
import { dirname, join } from "node:path";
import { gunzipSync } from "node:zlib";
import { Effect } from "effect";
import { CliUserError } from "../errors";
import { isCompiledBinary, tryDeltaUpgrade } from "./patch";
import {
  assetUrl,
  checksumsUrl,
  downloadBytes,
  downloadOptionalBytes,
  fetchText,
  gzAssetUrl,
  parseChecksum,
  type ReleaseConfig,
} from "./release";

const EXEC_MODE = 0o755;

/** Lowercase sha256 hex digest of `bytes`, via Bun's crypto hasher. */
export const sha256Hex = (bytes: Uint8Array): string =>
  new Bun.CryptoHasher("sha256").update(bytes).digest("hex");

/** A scratch path beside `targetPath`, so the later `rename` stays on one filesystem. */
const tempPathFor = (targetPath: string): string =>
  join(dirname(targetPath), `.peektrace-upgrade-${crypto.randomUUID()}.tmp`);

/** Remove a scratch file, ignoring a missing one. */
const discard = (tmpPath: string): Promise<void> =>
  unlink(tmpPath).catch(() => undefined);

/**
 * Move `tmpPath` over `targetPath` — a POSIX rename-over-self, safe while this
 * process runs since it keeps the old inode open. The exec bit is re-asserted
 * after the rename regardless of umask.
 */
const installFile = async (tmpPath: string, targetPath: string) => {
  await rename(tmpPath, targetPath);
  await chmod(targetPath, EXEC_MODE);
};

/**
 * Atomically replace `targetPath` with `bytes`. Writes a temp file in the same
 * directory, fsyncs it, then renames it over the target. `targetPath` defaults
 * to the running executable; tests point it at a temp file. The temp file is
 * removed on failure.
 */
export const replaceBinary = ({
  bytes,
  targetPath = process.execPath,
}: {
  readonly bytes: Uint8Array;
  readonly targetPath?: string;
}): Effect.Effect<void, CliUserError> =>
  Effect.tryPromise({
    try: async () => {
      const tmpPath = tempPathFor(targetPath);
      try {
        const handle = await open(tmpPath, "w", EXEC_MODE);
        try {
          await handle.write(bytes);
          await handle.sync();
        } finally {
          await handle.close();
        }
        await installFile(tmpPath, targetPath);
      } catch (cause) {
        await discard(tmpPath);
        throw cause;
      }
    },
    catch: (cause) =>
      new CliUserError({
        message: `Failed to replace ${targetPath}: ${String(cause)}`,
      }),
  });

/** Decompress gzipped release bytes, or `null` when they are not valid gzip. */
const tryGunzip = (bytes: Uint8Array): Uint8Array | null => {
  try {
    return new Uint8Array(gunzipSync(bytes));
  } catch {
    return null;
  }
};

/**
 * Download the binary for `tag`, preferring the `.gz` asset (about a third of
 * the raw size) and falling back to the uncompressed one. The fallback also
 * covers releases published before `.gz` assets existed, so pinning an old
 * version keeps working. Returns the *uncompressed* bytes either way.
 */
const downloadFullBinary = ({
  config,
  tag,
  asset,
}: {
  readonly config: ReleaseConfig;
  readonly tag: string;
  readonly asset: string;
}): Effect.Effect<Uint8Array, CliUserError> =>
  Effect.gen(function* () {
    const gz = yield* downloadOptionalBytes(gzAssetUrl(config, tag, asset));
    const unzipped = gz === null ? null : tryGunzip(gz);
    if (unzipped !== null) {
      return unzipped;
    }
    return yield* downloadBytes(assetUrl(config, tag, asset));
  });

/** The expected digest for `asset` in `tag`'s manifest, or a `CliUserError`. */
const expectedDigest = ({
  config,
  tag,
  asset,
}: {
  readonly config: ReleaseConfig;
  readonly tag: string;
  readonly asset: string;
}): Effect.Effect<string, CliUserError> =>
  Effect.gen(function* () {
    const sumsText = yield* fetchText(checksumsUrl(config, tag));
    const expected = parseChecksum(sumsText, asset);
    if (expected === null) {
      return yield* new CliUserError({
        message: `No SHA256SUMS entry for ${asset}; refusing to install.`,
      });
    }
    return expected;
  });

/** How the new binary was obtained, for the caller's progress message. */
export type UpgradeMethod =
  | { readonly _tag: "delta"; readonly patchBytes: number }
  | { readonly _tag: "full"; readonly bytes: number };

/**
 * Patch `targetPath` from `currentTag` up to `tag`, installing the result on
 * success. Resolves to `null` whenever no delta path applies — a release with
 * no patch chain, a network miss, a corrupt patch — leaving the caller to
 * download the release in full.
 *
 * `binpatch` verifies the produced binary against the target asset's sha256
 * digest before this ever sees it; the scratch file is removed on any failure.
 */
const tryDeltaInstall = ({
  config,
  tag,
  asset,
  currentTag,
  targetPath,
}: {
  readonly config: ReleaseConfig;
  readonly tag: string;
  readonly asset: string;
  readonly currentTag: string;
  readonly targetPath: string;
}): Effect.Effect<UpgradeMethod | null> =>
  Effect.gen(function* () {
    const tmpPath = tempPathFor(targetPath);
    const patchBytes = yield* tryDeltaUpgrade({
      config,
      asset,
      currentTag,
      targetTag: tag,
      oldPath: targetPath,
      destPath: tmpPath,
    });
    if (patchBytes !== null) {
      const installed = yield* Effect.tryPromise(() =>
        installFile(tmpPath, targetPath)
      ).pipe(Effect.option);
      if (installed._tag === "Some") {
        return { _tag: "delta", patchBytes } as const;
      }
    }
    yield* Effect.promise(() => discard(tmpPath));
    return null;
  });

/** `true` when `targetPath` is a real file a patch chain could be applied to. */
const isPatchable = (targetPath: string): Effect.Effect<boolean> =>
  Effect.tryPromise(async () => (await stat(targetPath)).isFile()).pipe(
    Effect.orElseSucceed(() => false)
  );

/**
 * The version to patch *from*, or `null` when patching does not apply: only a
 * compiled binary (under `bun run` the running executable is Bun itself, which
 * no chain applies to) sitting at a plain file can be patched.
 */
const deltaSourceTag = ({
  currentTag,
  targetPath,
}: {
  readonly currentTag: string | undefined;
  readonly targetPath: string;
}): Effect.Effect<string | null> =>
  Effect.gen(function* () {
    if (currentTag === undefined || !isCompiledBinary()) {
      return null;
    }
    return (yield* isPatchable(targetPath)) ? currentTag : null;
  });

/**
 * Bring `targetPath` up to `tag`: patch it from `currentTag` when a delta chain
 * is available, else download and verify the release asset in full. Aborts as a
 * `CliUserError` (installed binary untouched) on a missing manifest entry or a
 * checksum mismatch. Returns which route was taken.
 */
export const performUpgrade = ({
  config,
  tag,
  asset,
  currentTag,
  targetPath = process.execPath,
}: {
  readonly config: ReleaseConfig;
  readonly tag: string;
  readonly asset: string;
  readonly currentTag?: string;
  readonly targetPath?: string;
}): Effect.Effect<UpgradeMethod, CliUserError> =>
  Effect.gen(function* () {
    const from = yield* deltaSourceTag({ currentTag, targetPath });
    if (from !== null) {
      const delta = yield* tryDeltaInstall({
        config,
        tag,
        asset,
        currentTag: from,
        targetPath,
      });
      if (delta !== null) {
        return delta;
      }
    }

    const bytes = yield* downloadFullBinary({ config, tag, asset });
    const expected = yield* expectedDigest({ config, tag, asset });
    const actual = sha256Hex(bytes);
    if (actual !== expected) {
      return yield* new CliUserError({
        message: `Checksum mismatch for ${asset} (expected ${expected}, got ${actual}). Aborting without installing.`,
      });
    }
    yield* replaceBinary({ bytes, targetPath });
    return { _tag: "full", bytes: bytes.length } as const;
  });
