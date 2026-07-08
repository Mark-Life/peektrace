import { defineConfig, externalizeDepsPlugin } from "electron-vite";

// Electron's runtime is provided by the launcher binary, not the bundle.
// electron-log and friends ship native modules that also must stay external.
const ELECTRON_EXTERNALS = [
  "electron",
  "electron-log",
  "electron-log/main",
  "electron-store",
  "electron-window-state",
];

// Two targets only: the ESM main process and the CJS preload. There is no
// renderer bundle — the window loads the inspector served by the sidecar over
// http://127.0.0.1:<port>/, and the startup/crash screens are inline data: URLs.
export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    build: {
      rollupOptions: {
        input: { index: "src/main/index.ts" },
        external: ELECTRON_EXTERNALS,
      },
    },
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    build: {
      rollupOptions: {
        input: { index: "src/preload/index.ts" },
        external: ELECTRON_EXTERNALS,
        // Sandboxed preloads must be CommonJS.
        output: {
          format: "cjs",
          entryFileNames: "[name].js",
        },
      },
    },
  },
});
