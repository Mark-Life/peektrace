/** `peektrace upgrade` — self-update the installed CLI to the latest release.
 *
 * Mirrors `scripts/install.sh`: resolve the target `cli-v*` tag (pinned via
 * `--version`/`PEEKTRACE_VERSION` or the newest from the GitHub API), download the
 * host asset + `SHA256SUMS`, verify the sha256, then atomically replace the
 * running binary. `--check` reports availability and writes nothing. Windows
 * cannot replace a running `.exe` in place, so it punts to the PowerShell
 * installer with a clear message.
 */
import { Command, Options } from "@effect/cli";
import { Console, Effect, Option } from "effect";
import { CliUserError } from "../errors";
import { performUpgrade } from "../upgrade/install";
import {
  compareVersions,
  detectAsset,
  normalizeCliTag,
  resolveReleaseConfig,
  resolveTargetTag,
  SUPPORTED_PLATFORMS,
} from "../upgrade/release";
import { APP_VERSION } from "../version";

const WINDOWS_INSTALL_HINT =
  "Automatic in-place upgrade is not supported on Windows (a running .exe can't replace itself).\n" +
  "Re-run the installer to update:\n" +
  "  irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.ps1 | iex";

const versionOpt = Options.text("version").pipe(
  Options.withDescription(
    "Install a specific release tag instead of the latest (pin or downgrade)"
  ),
  Options.optional
);
const checkOpt = Options.boolean("check").pipe(
  Options.withDescription(
    "Only report whether an update is available; download and write nothing"
  ),
  Options.withDefault(false)
);

/** `upgrade` — download, verify, and atomically replace the installed binary. */
export const makeUpgrade = () =>
  Command.make(
    "upgrade",
    { version: versionOpt, check: checkOpt },
    ({ version, check }) =>
      Effect.gen(function* () {
        const config = resolveReleaseConfig();
        const detection = detectAsset(process.platform, process.arch);
        if (detection._tag === "unsupported") {
          return yield* new CliUserError({
            message: `${detection.reason}. Supported: ${SUPPORTED_PLATFORMS}.`,
          });
        }

        const pinned = Option.getOrUndefined(version);
        const targetTag = yield* resolveTargetTag(config, pinned);
        const comparison = compareVersions(targetTag, APP_VERSION);

        if (check) {
          if (comparison > 0) {
            yield* Console.log(
              `A newer version (${targetTag}) is available — run 'peektrace upgrade'`
            );
          } else {
            yield* Console.log(
              `peektrace is up to date (${normalizeCliTag(APP_VERSION)}).`
            );
          }
          return;
        }

        if (detection.os === "windows") {
          yield* Console.log(WINDOWS_INSTALL_HINT);
          return;
        }

        const forced =
          pinned !== undefined || config.pinnedVersion !== undefined;
        if (comparison === 0 && !forced) {
          yield* Console.log(`Already on the latest version (${targetTag}).`);
          return;
        }

        yield* Console.log(`Downloading ${detection.asset} (${targetTag})...`);
        yield* performUpgrade({
          config,
          tag: targetTag,
          asset: detection.asset,
        });
        yield* Console.log(
          `Upgraded to ${targetTag}. It takes effect on the next launch.`
        );
      })
  );
