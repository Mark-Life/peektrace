#!/usr/bin/env bun
/**
 * Stage the compiled `peektrace` CLI binary as the desktop sidecar.
 *
 * Runs as the electrobun `preBuild` hook, BEFORE electrobun's `copy` step, so the
 * staged binary in `resources/peektrace/` gets bundled into the app. It compiles
 * the CLI's own `src/build.ts` (a single self-contained Bun binary for the host,
 * or the `BUN_TARGET` cross target) and copies the result in with the exec bit.
 *
 * Dev builds skip all of this: `electrobun dev` sets ELECTROBUN_BUILD_ENV=dev, and
 * the desktop spawns the CLI via `bun run` instead of a compiled binary.
 */
import { spawnSync } from "node:child_process";
import { chmod, cp, mkdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";

const LOG = "[stage-sidecar]";
const EXEC_MODE = 0o755;

const SCRIPTS_DIR = import.meta.dirname;
const ROOT = resolve(SCRIPTS_DIR, "..");
const REPO_ROOT = resolve(ROOT, "..", "..");
const CLI_ROOT = join(REPO_ROOT, "apps", "cli");
const OUT_DIR = join(ROOT, "resources", "peektrace");

/** Bun compile targets keyed by `<platform>-<arch>`, matching the CLI's build.ts. */
const HOST_TARGETS: Record<string, string> = {
  "darwin-arm64": "bun-darwin-arm64",
  "darwin-x64": "bun-darwin-x64",
  "linux-x64": "bun-linux-x64",
  "linux-arm64": "bun-linux-arm64",
  "win32-x64": "bun-windows-x64",
  "win32-arm64": "bun-windows-arm64",
};

// electrobun exposes the build target via env; map it back to a Bun target when
// present, else fall back to the host. electrobun builds the current host only.
const ELECTROBUN_TARGETS: Record<string, string> = {
  "macos-arm64": "bun-darwin-arm64",
  "macos-x64": "bun-darwin-x64",
  "linux-arm64": "bun-linux-arm64",
  "linux-x64": "bun-linux-x64",
  "win-x64": "bun-windows-x64",
  "win-arm64": "bun-windows-arm64",
};

/** Resolve the Bun compile target from env, electrobun target, or host platform. */
const resolveTarget = (): string => {
  if (process.env.BUN_TARGET) {
    return process.env.BUN_TARGET;
  }
  const eb = `${process.env.ELECTROBUN_OS ?? ""}-${process.env.ELECTROBUN_ARCH ?? ""}`;
  if (ELECTROBUN_TARGETS[eb]) {
    return ELECTROBUN_TARGETS[eb];
  }
  const key = `${process.platform}-${process.arch}`;
  const target = HOST_TARGETS[key];
  if (!target) {
    throw new Error(`${LOG} unsupported host platform: ${key}`);
  }
  return target;
};

/** Compile the CLI binary via its own build script for `target`. */
const compileBinary = (target: string): void => {
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

/** Copy the compiled binary into `resources/peektrace/` with the exec bit set. */
const stageBinary = async (target: string): Promise<string> => {
  const name = target.includes("windows") ? "peektrace.exe" : "peektrace";
  const source = join(CLI_ROOT, "dist", target, name);
  const dest = join(OUT_DIR, name);

  await rm(OUT_DIR, { recursive: true, force: true });
  await mkdir(OUT_DIR, { recursive: true });
  await cp(source, dest);
  if (!target.includes("windows")) {
    await chmod(dest, EXEC_MODE);
  }
  return dest;
};

const main = async (): Promise<void> => {
  const env = process.env.ELECTROBUN_BUILD_ENV ?? "dev";
  if (env === "dev") {
    // Guarantee the `copy` source dir exists (it's gitignored, so absent on a
    // fresh clone). Dev spawns the CLI via `bun run`, so no binary is staged.
    await mkdir(OUT_DIR, { recursive: true });
    console.log(
      `${LOG} dev build — skipping sidecar compile (spawned via bun).`
    );
    return;
  }
  const target = resolveTarget();
  compileBinary(target);
  const dest = await stageBinary(target);
  console.log(`${LOG} staged sidecar at ${dest}`);
};

await main();
