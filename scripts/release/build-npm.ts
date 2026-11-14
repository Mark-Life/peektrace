#!/usr/bin/env bun
/**
 * Stage the publishable npm packages for the `peektrace` CLI from prebuilt
 * release binaries (the GitHub Release assets, uncompressed):
 *
 *   bun scripts/release/build-npm.ts --version <semver> --assets <dir>
 *     [--out <dir>] [--targets darwin-arm64,linux-x64,...]
 *
 * Lays out clean package dirs under `--out` (default `dist-npm/`):
 *
 *   peektrace/                    the unscoped wrapper: shim, postinstall, README,
 *                                 optionalDependencies pinning every variant
 *   @peektrace/cli-<platform>-<arch>/
 *     package.json                os/cpu-filtered, so npm installs only the host's
 *     bin/peektrace[.exe]
 *
 * The wrapper is unscoped so `npx peektrace` works; the variants live under the
 * `@peektrace` scope, which the shim builds from `os.platform()`/`os.arch()`.
 * `--targets` stages a subset (a local check on one host); the wrapper still
 * pins all five. Nothing is published here: publish every variant first, then
 * the wrapper, so its optionalDependencies resolve.
 */
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import {
  binaryName,
  DESCRIPTION,
  KEYWORDS,
  TARGETS,
  type Target,
  targetKey,
  variantName,
} from "./npm-meta";
import { npmReadme } from "./npm-readme";

const NPM_SRC = join(import.meta.dir, "npm");
const ROOT = resolve(import.meta.dir, "../..");
const ROOT_README = join(ROOT, "README.md");
const REPO_URL = "https://github.com/Mark-Life/peektrace";
/** rwxr-xr-x, so `npx` and global installs can exec the bin on unix. */
const EXECUTABLE_MODE = 0o755;
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

const commonMeta = {
  homepage: `${REPO_URL}#readme`,
  bugs: { url: `${REPO_URL}/issues` },
  repository: { type: "git", url: `git+${REPO_URL}.git` },
  license: "MIT",
} as const;

export interface StageOptions {
  readonly assets: string;
  readonly out: string;
  readonly readme: string;
  readonly targets: readonly Target[];
  readonly version: string;
}

const writeJson = (path: string, value: unknown) =>
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);

const stageVariant = (t: Target, { assets, out, version }: StageOptions) => {
  const source = join(assets, t.asset);
  if (!existsSync(source)) {
    throw new Error(`Missing release binary ${source}`);
  }
  const pkgDir = join(out, variantName(t));
  mkdirSync(join(pkgDir, "bin"), { recursive: true });
  const dest = join(pkgDir, "bin", binaryName(t));
  cpSync(source, dest);
  chmodSync(dest, EXECUTABLE_MODE);
  writeJson(join(pkgDir, "package.json"), {
    name: variantName(t),
    version,
    description: `${DESCRIPTION} (${targetKey(t)} binary)`,
    os: [t.platform],
    cpu: [t.arch],
    files: ["bin"],
    ...commonMeta,
  });
  return pkgDir;
};

const stageWrapper = ({ out, readme, version }: StageOptions) => {
  const pkgDir = join(out, "peektrace");
  mkdirSync(pkgDir, { recursive: true });
  writeJson(join(pkgDir, "package.json"), {
    name: "peektrace",
    version,
    description: DESCRIPTION,
    keywords: [...KEYWORDS],
    ...commonMeta,
    type: "commonjs",
    bin: { peektrace: "./peektrace.js" },
    scripts: { postinstall: "node postinstall.js" },
    files: ["peektrace.js", "postinstall.js", "README.md"],
    optionalDependencies: Object.fromEntries(
      TARGETS.map((t) => [variantName(t), version])
    ),
    engines: { node: ">=20" },
  });
  const shim = join(pkgDir, "peektrace.js");
  cpSync(join(NPM_SRC, "peektrace.js"), shim);
  chmodSync(shim, EXECUTABLE_MODE);
  cpSync(join(NPM_SRC, "postinstall.js"), join(pkgDir, "postinstall.js"));
  writeFileSync(join(pkgDir, "README.md"), npmReadme(readme, `cli-v${version}`));
  return pkgDir;
};

/** Wipe `out`, then stage every variant in `targets` and the wrapper. */
export const stage = (options: StageOptions) => {
  if (!SEMVER.test(options.version)) {
    throw new Error(`Not a semver version: ${options.version}`);
  }
  rmSync(options.out, { recursive: true, force: true });
  mkdirSync(options.out, { recursive: true });
  const variants = options.targets.map((t) => stageVariant(t, options));
  return { variants, wrapper: stageWrapper(options) };
};

/** The `--targets` list as targets; every target when empty. */
export const pickTargets = (list: string | undefined) => {
  if (!list) {
    return TARGETS;
  }
  return list.split(",").map((key) => {
    const found = TARGETS.find((t) => targetKey(t) === key.trim());
    if (!found) {
      const known = TARGETS.map(targetKey).join(", ");
      throw new Error(`Unknown target ${key} (known: ${known})`);
    }
    return found;
  });
};

const main = () => {
  const { values } = parseArgs({
    options: {
      version: { type: "string" },
      assets: { type: "string" },
      out: { type: "string", default: join(ROOT, "dist-npm") },
      targets: { type: "string" },
    },
  });
  if (!(values.version && values.assets)) {
    console.error(
      "usage: build-npm.ts --version <semver> --assets <dir> [--out <dir>] [--targets a,b]"
    );
    process.exit(1);
  }
  const staged = stage({
    version: values.version,
    assets: resolve(values.assets),
    out: resolve(values.out),
    targets: pickTargets(values.targets),
    readme: readFileSync(ROOT_README, "utf8"),
  });
  for (const dir of [...staged.variants, staged.wrapper]) {
    console.log(`Staged ${dir}`);
  }
};

if (import.meta.main) {
  main();
}
