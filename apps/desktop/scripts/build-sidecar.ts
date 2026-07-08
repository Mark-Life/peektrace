#!/usr/bin/env bun
/**
 * Stage the compiled `peephole` CLI binary as the desktop sidecar.
 *
 * Runs the CLI's own `src/build.ts` (which compiles a single self-contained Bun
 * binary for the host, or the `BUN_TARGET` cross target so CI can drive one leg
 * per arch), then copies the emitted binary into `resources/peephole/` and sets
 * the unix exec bit. electron-builder later ships that directory verbatim as an
 * `extraResources` entry — outside the asar, since asar-packed files can't be
 * exec'd.
 *
 * Runs as the desktop `prebuild`/`build` step, before `electron-vite build`.
 */
import { spawnSync } from "node:child_process";
import { chmod, cp, mkdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";

const LOG = "[build-sidecar]";
const EXEC_MODE = 0o755;

const SCRIPTS_DIR = import.meta.dirname;
const ROOT = resolve(SCRIPTS_DIR, "..");
const REPO_ROOT = resolve(ROOT, "..", "..");
const CLI_ROOT = join(REPO_ROOT, "apps", "cli");
const OUT_DIR = join(ROOT, "resources", "peephole");

/**
 * Bun compile target strings keyed by `<platform>-<arch>`. Kept in sync with the
 * CLI's `src/build.ts` so the staged binary is copied from the same `dist/`
 * subdirectory the compile writes to.
 */
const HOST_TARGETS: Record<string, string> = {
  "darwin-arm64": "bun-darwin-arm64",
  "darwin-x64": "bun-darwin-x64",
  "linux-x64": "bun-linux-x64",
  "linux-arm64": "bun-linux-arm64",
  "win32-x64": "bun-windows-x64",
  "win32-arm64": "bun-windows-arm64",
};

/** Resolve the Bun compile target from `BUN_TARGET` or the host platform. */
const resolveTarget = () => {
  const fromEnv = process.env.BUN_TARGET;
  if (fromEnv) {
    return fromEnv;
  }
  const key = `${process.platform}-${process.arch}`;
  const target = HOST_TARGETS[key];
  if (!target) {
    throw new Error(`${LOG} unsupported host platform: ${key}`);
  }
  return target;
};

/** Compile the CLI binary via its own build script for `target`. */
const compileBinary = (target: string) => {
  console.log(`${LOG} compiling CLI binary for ${target}…`);
  const result = spawnSync("bun", ["run", join("src", "build.ts")], {
    cwd: CLI_ROOT,
    stdio: ["ignore", "inherit", "inherit"],
    env: { ...process.env, BUN_TARGET: target },
  });
  if (result.status !== 0) {
    throw new Error(`${LOG} CLI binary build failed (exit ${result.status})`);
  }
};

/** Copy the compiled binary (plus any sibling assets) into `resources/peephole/`. */
const stageBinary = async (target: string) => {
  const binaryName = target.includes("windows") ? "peephole.exe" : "peephole";
  const source = join(CLI_ROOT, "dist", target, binaryName);
  const dest = join(OUT_DIR, binaryName);

  await rm(OUT_DIR, { recursive: true, force: true });
  await mkdir(OUT_DIR, { recursive: true });
  await cp(source, dest);

  // Skip chmod for `.exe` cross-builds — the target has no unix exec bit and the
  // path may not exist on a Windows host.
  if (!target.includes("windows")) {
    await chmod(dest, EXEC_MODE);
  }
  return dest;
};

const main = async () => {
  const target = resolveTarget();
  compileBinary(target);
  const dest = await stageBinary(target);
  console.log(`${LOG} staged sidecar at ${dest}`);
};

await main();
