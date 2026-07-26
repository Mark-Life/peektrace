/** Read + write the user settings file (`~/.peektrace/settings.json`).
 *
 * Server-only: it touches `node:*` (path/crypto) and the platform FileSystem, so
 * it is never pulled into the browser bundle (the RPC contract imports only the
 * pure `PeektraceSettings` schema from `./settings`). Writes go straight to the
 * fixed, trusted settings path via a temp-file + rename, bypassing the agent-root
 * `WriteFs` guard (that guard exists to contain writes to a *user-supplied* path;
 * the settings path is fixed, so containment adds nothing here).
 *
 * NOTE: the running server resolves agent roots once at boot, so a write here
 * only changes the scanned session list after `peektrace serve` restarts. `get`
 * always reads the file fresh, so the editor reflects the file's true contents.
 */
import { randomBytes } from "node:crypto";
import { dirname, join } from "node:path";
import { FileSystem } from "@effect/platform";
import { Context, Data, Effect, Layer, Option } from "effect";
import { settingsPath } from "./agents";
import { FileChangedError } from "./fs";
import { type PeektraceSettings, parseSettings } from "./settings";

/** Raised when the settings file cannot be written (IO failure). */
export class SettingsWriteError extends Data.TaggedError("SettingsWriteError")<{
  readonly path: string;
  readonly reason: string;
}> {}

/** The current settings on disk plus the mtime used for compare-and-swap. */
export interface SettingsResult {
  /** File mtime in ms (0 when the file does not exist yet). */
  readonly mtimeMs: number;
  /** Absolute path of the settings file (shown in the UI). */
  readonly path: string;
  readonly settings: PeektraceSettings;
}

/** Service contract: read + atomically write the settings file. */
export interface SettingsServiceShape {
  /** Read + parse the settings file fresh (empty when absent). */
  readonly get: Effect.Effect<SettingsResult>;
  /** Validate-and-write the settings file, with optional mtime CAS. */
  readonly update: (args: {
    readonly settings: PeektraceSettings;
    readonly expectedMtime?: number;
  }) => Effect.Effect<SettingsResult, SettingsWriteError | FileChangedError>;
}

/** Read/write access to `~/.peektrace/settings.json`. */
export class SettingsService extends Context.Tag("@peektrace/SettingsService")<
  SettingsService,
  SettingsServiceShape
>() {}

const TEMP_TOKEN_BYTES = 6;

/** File mtime in ms, or 0 when the file is missing/unstattable. */
const statMtime = (fs: FileSystem.FileSystem, path: string) =>
  fs.stat(path).pipe(
    Effect.map((info) =>
      Option.match(info.mtime, {
        onNone: () => 0,
        onSome: (d) => d.getTime(),
      })
    ),
    Effect.orElseSucceed(() => 0)
  );

const makeService = (fs: FileSystem.FileSystem): SettingsServiceShape => {
  const get = Effect.gen(function* () {
    const path = settingsPath();
    // Stat BEFORE reading so the CAS token is never newer than the content it
    // labels: a write landing in the read/stat window then yields a stale token
    // that fails CAS on save (a safe refresh) rather than a silent lost update.
    const mtimeMs = yield* statMtime(fs, path);
    const raw = yield* fs
      .readFileString(path)
      .pipe(Effect.orElseSucceed(() => ""));
    return {
      settings: parseSettings(raw),
      mtimeMs,
      path,
    } satisfies SettingsResult;
  });

  /** Fail with `FileChangedError` when the on-disk mtime no longer matches. */
  const checkCas = (path: string, expectedMtime: number | undefined) =>
    Effect.gen(function* () {
      if (expectedMtime === undefined) {
        return;
      }
      const exists = yield* fs
        .exists(path)
        .pipe(Effect.orElseSucceed(() => false));
      if (!exists) {
        if (expectedMtime !== 0) {
          yield* Effect.fail(new FileChangedError({ path, reason: "missing" }));
        }
        return;
      }
      const current = yield* statMtime(fs, path);
      if (current !== expectedMtime) {
        yield* Effect.fail(new FileChangedError({ path, reason: "mtime" }));
      }
    });

  const update: SettingsServiceShape["update"] = ({
    settings,
    expectedMtime,
  }) =>
    Effect.gen(function* () {
      const path = settingsPath();
      yield* checkCas(path, expectedMtime);
      const dir = dirname(path);
      const content = `${JSON.stringify(settings, null, 2)}\n`;
      const token = randomBytes(TEMP_TOKEN_BYTES).toString("hex");
      const tempPath = join(dir, `.settings.json.tmp-${token}`);
      yield* fs.makeDirectory(dir, { recursive: true }).pipe(Effect.ignore);
      yield* fs.writeFileString(tempPath, content).pipe(
        Effect.flatMap(() => fs.rename(tempPath, path)),
        Effect.mapError(
          (e) => new SettingsWriteError({ path, reason: String(e) })
        )
      );
      const mtimeMs = yield* statMtime(fs, path);
      return { settings, mtimeMs, path } satisfies SettingsResult;
    }).pipe(
      Effect.withSpan("Settings.update", { attributes: { expectedMtime } })
    );

  return { get, update };
};

/** Live layer over the platform FileSystem. */
export const SettingsServiceLive = Layer.effect(
  SettingsService,
  Effect.map(FileSystem.FileSystem, makeService)
);
