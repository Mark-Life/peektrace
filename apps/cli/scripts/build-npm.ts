#!/usr/bin/env bun
/**
 * Stage the publishable npm packages for the `peektrace` CLI.
 *
 * Cross-compiles the binary for every supported target by re-running the Phase A
 * compiler (`src/build.ts`) once per `BUN_TARGET`, then lays out a set of clean,
 * publishable package directories under the gitignored `dist-npm/`:
 *
 *   dist-npm/
 *     peektrace/                     -> the main wrapper (name: "peektrace")
 *       package.json                   bin -> ./peektrace.js, os/cpu-filtered
 *       peektrace.js                    optionalDependencies + a postinstall check
 *       postinstall.js
 *       README.md
 *     peektrace-darwin-arm64/    -> one per-platform binary package each, named
 *       package.json                   peektrace-<platform>-<arch> with matching
 *       bin/peektrace                   os/cpu so npm installs only the host's variant
 *     peektrace-darwin-x64/ ...
 *     peektrace-linux-x64/ ...
 *     peektrace-win32-x64/ ...
 *
 * Nothing here is published or logged into: this only builds + stages. `npm publish`
 * is a separate CI/manual step (publish each `peektrace-*` variant first, then the
 * `peektrace` wrapper, so its optionalDependencies resolve).
 *
 * Naming is UNSCOPED (`peektrace`, `peektrace-<platform>-<arch>`) to avoid needing
 * an npm org. The documented alternative is a scoped `@peektrace/*` family (main
 * `@peektrace/cli`, variants `@peektrace/cli-<platform>-<arch>`); switching would only
 * change the name strings below plus the shim's `packageName` and needs an npm org.
 *
 * The in-monorepo dev workflow is untouched: `apps/cli/package.json` stays private
 * with `bin -> ./src/index.ts`; the shipped wrapper is these generated dirs, never
 * `apps/cli` itself.
 */
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";

const SCRIPT_DIR = import.meta.dirname;
const CLI_ROOT = resolve(SCRIPT_DIR, "..");
const NPM_SRC = join(SCRIPT_DIR, "npm");
const DIST_NPM = join(CLI_ROOT, "dist-npm");
const CLI_PKG_JSON = join(CLI_ROOT, "package.json");
const CLI_README = join(CLI_ROOT, "README.md");

const REPO_URL = "https://github.com/Mark-Life/peektrace";
const LICENSE = "MIT";
/** rwxr-xr-x — needed so `npx`/global installs can exec the bin on unix. */
const EXECUTABLE_MODE = 0o755;
const DESCRIPTION =
  "Local, loopback-only inspector for Claude Code memories & sessions.";

/** One publishable per-platform binary variant. */
interface Target {
  readonly arch: "x64" | "arm64";
  readonly bunTarget: Bun.Build.CompileTarget;
  readonly platform: "darwin" | "linux" | "win32";
}

/** The matrix of platforms shipped as `peektrace-<platform>-<arch>` packages. */
const TARGETS = [
  { platform: "darwin", arch: "arm64", bunTarget: "bun-darwin-arm64" },
  { platform: "darwin", arch: "x64", bunTarget: "bun-darwin-x64" },
  { platform: "linux", arch: "x64", bunTarget: "bun-linux-x64" },
  { platform: "win32", arch: "x64", bunTarget: "bun-windows-x64" },
] as const satisfies readonly Target[];

/** npm package name for a per-platform binary variant. */
const variantName = (t: Target) => `peektrace-${t.platform}-${t.arch}`;

/** Binary filename inside a variant (`.exe` only on Windows). */
const binaryName = (t: Target) =>
  t.platform === "win32" ? "peektrace.exe" : "peektrace";

/** Shared repository/homepage/bugs/license metadata for every emitted package. */
const commonMeta = {
  homepage: `${REPO_URL}#readme`,
  bugs: { url: `${REPO_URL}/issues` },
  repository: { type: "git", url: `git+${REPO_URL}.git` },
  license: LICENSE,
} as const;

/** Read the single source-of-truth version from `apps/cli/package.json`. */
const readVersion = async () => {
  const pkg = (await Bun.file(CLI_PKG_JSON).json()) as { version?: string };
  if (!pkg.version) {
    throw new Error(`No version in ${CLI_PKG_JSON}`);
  }
  return pkg.version;
};

/** Cross-compile one target via `src/build.ts`; returns the produced binary path. */
const compileTarget = (t: Target) => {
  console.log(`\n=== Building ${t.bunTarget} ===`);
  const result = spawnSync("bun", ["run", "src/build.ts"], {
    cwd: CLI_ROOT,
    env: { ...process.env, BUN_TARGET: t.bunTarget },
    stdio: ["ignore", "inherit", "inherit"],
  });
  if (result.status !== 0) {
    throw new Error(`Build failed for ${t.bunTarget} (exit ${result.status})`);
  }
  const outDir = join(CLI_ROOT, "dist", t.bunTarget);
  // Bun appends `.exe` for the Windows target; accept either name.
  const withExe = join(outDir, "peektrace.exe");
  const plain = join(outDir, "peektrace");
  const produced = existsSync(withExe) ? withExe : plain;
  if (!existsSync(produced)) {
    throw new Error(`Compiler produced no binary in ${outDir}`);
  }
  return produced;
};

/** Stage `dist-npm/peektrace-<platform>-<arch>/` with its binary + package.json. */
const stageVariant = (t: Target, version: string, producedBinary: string) => {
  const name = variantName(t);
  const pkgDir = join(DIST_NPM, name);
  const binDir = join(pkgDir, "bin");
  mkdirSync(binDir, { recursive: true });

  const dest = join(binDir, binaryName(t));
  cpSync(producedBinary, dest);
  chmodSync(dest, EXECUTABLE_MODE);

  const pkg = {
    name,
    version,
    description: `${DESCRIPTION} (${t.platform}-${t.arch} binary)`,
    // npm installs an optional dependency only when host os/cpu match, so every
    // non-host variant is skipped as a no-op — one wrapper ships all platforms.
    os: [t.platform],
    cpu: [t.arch],
    files: ["bin"],
    ...commonMeta,
  };
  writeFileSync(
    join(pkgDir, "package.json"),
    `${JSON.stringify(pkg, null, 2)}\n`
  );
  console.log(`Staged ${name} (${dest})`);
};

/** Stage the main `dist-npm/peektrace/` wrapper: package.json + shim + postinstall. */
const stageWrapper = (version: string) => {
  const pkgDir = join(DIST_NPM, "peektrace");
  mkdirSync(pkgDir, { recursive: true });

  const optionalDependencies = Object.fromEntries(
    TARGETS.map((t) => [variantName(t), version])
  );
  const pkg = {
    name: "peektrace",
    version,
    description: DESCRIPTION,
    keywords: [
      "claude",
      "claude-code",
      "inspector",
      "cli",
      "memory",
      "sessions",
    ],
    ...commonMeta,
    type: "commonjs",
    bin: { peektrace: "./peektrace.js" },
    scripts: { postinstall: "node postinstall.js" },
    files: ["peektrace.js", "postinstall.js", "README.md"],
    optionalDependencies,
    engines: { node: ">=20" },
  };
  writeFileSync(
    join(pkgDir, "package.json"),
    `${JSON.stringify(pkg, null, 2)}\n`
  );

  const shim = join(pkgDir, "peektrace.js");
  cpSync(join(NPM_SRC, "peektrace.js"), shim);
  chmodSync(shim, EXECUTABLE_MODE);
  cpSync(join(NPM_SRC, "postinstall.js"), join(pkgDir, "postinstall.js"));

  const readmeSrc = join(NPM_SRC, "README.md");
  cpSync(
    existsSync(readmeSrc) ? readmeSrc : CLI_README,
    join(pkgDir, "README.md")
  );
  console.log(`Staged peektrace wrapper (${pkgDir})`);
};

const main = async () => {
  const version = await readVersion();
  console.log(`Staging peektrace npm packages @ ${version}`);
  rmSync(DIST_NPM, { recursive: true, force: true });
  mkdirSync(DIST_NPM, { recursive: true });

  for (const t of TARGETS) {
    const produced = compileTarget(t);
    stageVariant(t, version, produced);
  }
  stageWrapper(version);

  console.log(`\nDone. Publishable packages staged under ${DIST_NPM}`);
  console.log(
    "Publish order (manual/CI): each peektrace-* first, then peektrace."
  );
};

await main();
