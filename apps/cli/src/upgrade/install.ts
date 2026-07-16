/** Verify-then-install for `peektrace upgrade`.
 *
 * Downloads the release asset + its `SHA256SUMS`, verifies the sha256 digest
 * against the manifest entry (mirroring `scripts/install.sh`' `verify_checksum`),
 * and replaces the running binary atomically. A checksum mismatch or a missing
 * manifest entry aborts *without* touching the installed binary.
 */
import { chmod, open, rename, unlink } from "node:fs/promises";
import { dirname, join } from "node:path";
import { Effect } from "effect";
import { CliUserError } from "../errors";
import {
  assetUrl,
  checksumsUrl,
  downloadBytes,
  fetchText,
  parseChecksum,
  type ReleaseConfig,
} from "./release";

const EXEC_MODE = 0o755;

/** Lowercase sha256 hex digest of `bytes`, via Bun's crypto hasher. */
export const sha256Hex = (bytes: Uint8Array): string =>
  new Bun.CryptoHasher("sha256").update(bytes).digest("hex");

/**
 * Atomically replace `targetPath` with `bytes`. Writes a temp file in the *same*
 * directory (so the following `rename` stays on one filesystem), fsyncs it, then
 * renames over the target — a POSIX rename-over-self, safe while this process
 * runs since it keeps the old inode open. The exec bit is set on the temp file
 * and re-asserted after the rename. `targetPath` defaults to the running
 * executable; tests point it at a temp file. The temp file is removed on failure.
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
      const tmpPath = join(
        dirname(targetPath),
        `.peektrace-upgrade-${crypto.randomUUID()}.tmp`
      );
      try {
        const handle = await open(tmpPath, "w", EXEC_MODE);
        try {
          await handle.write(bytes);
          await handle.sync();
        } finally {
          await handle.close();
        }
        await rename(tmpPath, targetPath);
        await chmod(targetPath, EXEC_MODE);
      } catch (cause) {
        await unlink(tmpPath).catch(() => {
          /* temp may not exist; nothing to clean up */
        });
        throw cause;
      }
    },
    catch: (cause) =>
      new CliUserError({
        message: `Failed to replace ${targetPath}: ${String(cause)}`,
      }),
  });

/**
 * Download the asset for `tag`, verify its sha256 against `SHA256SUMS`, and
 * atomically replace `targetPath`. Aborts as a `CliUserError` (installed binary
 * untouched) on a missing manifest entry or a checksum mismatch.
 */
export const performUpgrade = ({
  config,
  tag,
  asset,
  targetPath,
}: {
  readonly config: ReleaseConfig;
  readonly tag: string;
  readonly asset: string;
  readonly targetPath?: string;
}): Effect.Effect<void, CliUserError> =>
  Effect.gen(function* () {
    const bytes = yield* downloadBytes(assetUrl(config, tag, asset));
    const sumsText = yield* fetchText(checksumsUrl(config, tag));
    const expected = parseChecksum(sumsText, asset);
    if (expected === null) {
      return yield* new CliUserError({
        message: `No SHA256SUMS entry for ${asset}; refusing to install.`,
      });
    }
    const actual = sha256Hex(bytes);
    if (actual !== expected) {
      return yield* new CliUserError({
        message: `Checksum mismatch for ${asset} (expected ${expected}, got ${actual}). Aborting without installing.`,
      });
    }
    yield* replaceBinary(
      targetPath === undefined ? { bytes } : { bytes, targetPath }
    );
  });
