#!/usr/bin/env node

/**
 * Best-effort install check for the `peektrace` npm wrapper.
 *
 * Warns (never fails) when no per-platform binary package
 * (`peektrace-<platform>-<arch>`) resolves for the host, so a user on an
 * unsupported platform gets a clear message at install time instead of a cryptic
 * error on first run. Always exits 0: a non-zero postinstall would abort the whole
 * install, and under bun postinstalls are disabled by default anyway (the runtime
 * resolution in `peektrace.js` is the real safety net).
 */

const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

try {
  const platform = os.platform();
  const arch = os.arch();
  const binary = platform === "win32" ? "peektrace.exe" : "peektrace";
  const packageName = "peektrace-" + platform + "-" + arch;
  const pkgJson = require.resolve(packageName + "/package.json");
  const candidate = path.join(path.dirname(pkgJson), "bin", binary);
  if (!fs.existsSync(candidate)) {
    console.warn(
      "peektrace: installed, but the platform binary for " +
        platform +
        "-" +
        arch +
        " looks incomplete. Run `peektrace doctor` if commands fail."
    );
  }
} catch {
  console.warn(
    "peektrace: no prebuilt binary was installed for " +
      os.platform() +
      "-" +
      os.arch() +
      ". This platform may be unsupported; peektrace commands will not run."
  );
}

process.exit(0);
