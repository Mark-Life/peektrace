/**
 * Which missing binaries the report's host has now, so the command-not-found
 * fix stops telling the user to install what they already installed.
 *
 * Only the dirs every login PATH holds count: a binary found there cannot be
 * "command not found" in a later shell. One that sits only in a user dir
 * (`~/.local/bin`, `~/.cargo/bin`) may still be off the agent's PATH, so the
 * fix keeps naming it. Read through the platform FileSystem, never the
 * process env: a remote report reads the server host's dirs.
 */
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";

/** Dirs on every PATH: Debian/Ubuntu, Fedora, macOS and Homebrew's. */
const SYSTEM_BIN_DIRS: readonly string[] = [
  "/usr/local/sbin",
  "/usr/local/bin",
  "/usr/sbin",
  "/usr/bin",
  "/sbin",
  "/bin",
  "/opt/homebrew/bin",
];

/** Binaries checked per report: the fix names at most a few. */
const CHECK_CAP = 32;

/** A name that can be a file in a bin dir: no path, no glob, no space. */
const PLAIN_NAME = /^[\w.+-]+$/;

/** The names of `bins` that a system bin dir holds. */
export const hostBinsOf = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly bins: readonly string[];
}): Effect.Effect<readonly string[]> =>
  Effect.filter(
    args.bins.filter((bin) => PLAIN_NAME.test(bin)).slice(0, CHECK_CAP),
    (bin) =>
      Effect.exists(SYSTEM_BIN_DIRS, (dir) =>
        args.fs.exists(`${dir}/${bin}`).pipe(Effect.orElseSucceed(() => false))
      )
  ).pipe(Effect.withSpan("Stats.hostBins"));
