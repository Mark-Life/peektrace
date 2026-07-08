#!/usr/bin/env node

/**
 * Published `bin` launcher for the `peephole` npm package.
 *
 * This is NOT the in-monorepo dev entry (`src/index.ts`). It ships only inside
 * the generated `dist-npm/peephole/` wrapper and is staged there by
 * `scripts/build-npm.ts`. Zero runtime dependencies: pure Node CommonJS using
 * only `child_process`/`fs`/`path`/`os`, so it runs under npm, bun and pnpm.
 *
 * Binary selection: the real compiled executable lives in a per-platform
 * optional dependency named `peephole-cli-<platform>-<arch>` (e.g.
 * `peephole-cli-darwin-arm64`). npm/bun install only the variant whose `os`/`cpu`
 * match the host and skip the rest, so exactly one is present. We `require.resolve`
 * it and spawn `bin/peephole` (or `bin/peephole.exe` on Windows), forwarding argv,
 * inheriting stdio and propagating the exit code/signal.
 *
 * Package names use the raw `os.platform()` value (`win32`, not `windows`), which
 * matches npm's `os` field, so there is no platform-name split-brain to maintain.
 */

const childProcess = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

/** Spawn `target`, forward argv/stdio/signals, and exit with the child's code. */
function spawnAndExit(target) {
  const child = childProcess.spawn(target, process.argv.slice(2), {
    stdio: "inherit",
  });
  child.on("error", (err) => {
    console.error(err.message);
    process.exit(1);
  });
  const forward = (signal) => {
    if (!child.killed) {
      try {
        child.kill(signal);
      } catch {}
    }
  };
  ["SIGINT", "SIGTERM", "SIGHUP"].forEach((sig) =>
    process.on(sig, () => forward(sig))
  );
  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
    } else {
      process.exit(typeof code === "number" ? code : 0);
    }
  });
}

/** Detect the package manager for a helpful reinstall hint. */
function detectPackageManager() {
  const ua = process.env.npm_config_user_agent || "";
  if (/\bbun\//.test(ua)) {
    return "bun";
  }
  const execPath = process.env.npm_execpath || "";
  if (execPath.includes("bun")) {
    return "bun";
  }
  if (
    __dirname.includes(".bun/install/global") ||
    __dirname.includes(".bun\\install\\global")
  ) {
    return "bun";
  }
  return ua ? "npm" : null;
}

/** Resolve the host's binary and hand off; exits the process (never returns). */
function main() {
  // Escape hatch: point at an explicit binary (useful for local testing / VPS).
  if (process.env.PEEPHOLE_BIN_PATH) {
    spawnAndExit(process.env.PEEPHOLE_BIN_PATH);
    return;
  }

  const binary = process.platform === "win32" ? "peephole.exe" : "peephole";
  const platform = os.platform();
  const arch = os.arch();
  const packageName = "peephole-cli-" + platform + "-" + arch;

  try {
    const pkgJson = require.resolve(packageName + "/package.json");
    const candidate = path.join(path.dirname(pkgJson), "bin", binary);
    if (fs.existsSync(candidate)) {
      spawnAndExit(candidate);
      return;
    }
  } catch {
    // The matching per-platform optional dependency is not installed; fall
    // through to the error below.
  }

  const pm = detectPackageManager();
  const reinstall =
    pm === "bun"
      ? "bun install -g peephole"
      : pm === "npm"
        ? "npm install -g peephole"
        : "reinstall peephole";
  console.error(
    "peephole: could not locate a platform binary for " +
      platform +
      "-" +
      arch +
      ".\n" +
      'Expected optional dependency: "' +
      packageName +
      '"\n' +
      "To fix: " +
      reinstall
  );
  process.exit(1);
}

main();
