import { readFileSync } from "node:fs";
import type { ElectrobunConfig } from "electrobun";

// Version tracks the desktop package so a single source (package.json) drives the
// bundle version, Info.plist, and the updater's local version.json.
const pkg = JSON.parse(
  readFileSync(new URL("./package.json", import.meta.url), "utf8")
) as { version: string };

export default {
  app: {
    name: "Peektrace",
    identifier: "com.mark-life.peektrace",
    version: pkg.version,
  },
  runtime: {
    // We manage quit + sidecar teardown ourselves via the before-quit event, but
    // quitting when the only window closes matches the previous Electron UX.
    exitOnLastWindowClosed: true,
  },
  build: {
    bun: {
      entrypoint: "src/bun/index.ts",
    },
    views: {
      // The crash screen is a bundled, RPC-capable view. The inspector UI itself
      // is served by the sidecar over http and loaded by URL, so it needs no
      // view entry here.
      crash: {
        entrypoint: "src/views/crash/index.ts",
      },
    },
    copy: {
      "src/views/crash/index.html": "views/crash/index.html",
      "src/views/crash/index.css": "views/crash/index.css",
      // The compiled `peektrace` CLI binary, staged into resources/ by the
      // preBuild hook (canary/stable only). Lands at <Resources>/app/peektrace/.
      "resources/peektrace": "peektrace",
    },
    // System WebView on every platform — the whole point of the swap. No CEF.
    mac: {
      bundleCEF: false,
      // Signing/notarization are off until an identity is wired in CI, mirroring
      // the previous unsigned electron-builder config.
      codesign: false,
      notarize: false,
      entitlements: {},
    },
    win: {
      bundleCEF: false,
    },
    linux: {
      bundleCEF: false,
      icon: "assets/icon.png",
    },
  },
  // Auto-update artifacts are served from a static host. Left empty until the
  // release pipeline publishes an `artifacts/` folder somewhere; the in-app
  // updater is a friendly no-op while this is unset (see src/bun/updater.ts).
  release: {
    baseUrl: process.env.PEEKTRACE_RELEASE_BASE_URL ?? "",
  },
  scripts: {
    // Compile + stage the CLI sidecar binary BEFORE electrobun's copy step so it
    // gets bundled. Skips work in dev (dev spawns the CLI via `bun run`).
    preBuild: "./scripts/stage-sidecar.ts",
  },
} satisfies ElectrobunConfig;
