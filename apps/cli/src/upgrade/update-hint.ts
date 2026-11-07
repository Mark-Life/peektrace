/** Pure helpers for the "newer version" hint: dev builds, install kind, text. */

const DEV_VERSION = /-dev(\.|$)/;
const PATH_SEPARATOR = /[\\/]/;

/** `0.0.0-dev` and `1.2.0-dev.3` style builds run from source. */
export const isDevVersion = (version: string): boolean =>
  DEV_VERSION.test(version);

/** True when the running binary sits under a `node_modules` directory (npm, npx, pnpm, bun global). */
export const isNpmInstall = (execPath: string): boolean =>
  execPath.split(PATH_SEPARATOR).includes("node_modules");

/** The upgrade hint matching how peektrace was installed. */
export const updateHint = ({
  latest,
  execPath,
}: {
  readonly latest: string;
  readonly execPath: string;
}): string =>
  isNpmInstall(execPath)
    ? `A newer version (${latest}) is available — run 'npm i -g peektrace@latest' (or 'npx peektrace@latest')`
    : `A newer version (${latest}) is available — run 'peektrace upgrade'`;
