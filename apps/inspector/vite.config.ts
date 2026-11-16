import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/**
 * Inspector Vite config.
 *
 * - Tailwind v4 via `@tailwindcss/vite` (styles come from
 *   `@workspace/ui/globals.css`, imported in `main.tsx`).
 * - Dev transport: proxy `/api` (JSON routes and the `/api/events` stream) to
 *   a running `peektrace serve`, so the dev server and the production
 *   (same-origin) build hit identical paths. Override the target with
 *   `PEEKTRACE_API_TARGET` (default `http://127.0.0.1:4321`).
 * - Prod transport: none needed — `peektrace serve` hosts `dist/` and `/api`
 *   on the same origin, so the default base URL `""` resolves `/api` directly.
 */
const API_TARGET =
  process.env.PEEKTRACE_API_TARGET ??
  process.env.PEEKTRACE_RPC_TARGET ??
  "http://127.0.0.1:4321";

const EFFECT_RE = /node_modules\/(effect|@effect|@effect-atom)\//;
const REACT_RE = /node_modules\/(react|react-dom|scheduler)\//;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    // Workspace packages carry their own `react` dependency, so a version drift
    // between them and this app resolves to two React copies. The second one
    // has a null dispatcher and every hook call throws at render time.
    dedupe: ["react", "react-dom"],
  },
  server: {
    proxy: {
      "/api": {
        target: API_TARGET,
        changeOrigin: true,
        // The server only accepts its own origin; the dev origin differs.
        configure: (proxy) => {
          proxy.on("proxyReq", (req) => req.removeHeader("origin"));
        },
      },
    },
  },
  build: {
    rollupOptions: {
      output: {
        // Split the two heavy, rarely-changing vendor trees every route needs
        // out of the app chunk, so a small UI edit doesn't bust their caches.
        //
        // Only trees the *first paint* already pulls in whole are named. A rule
        // that names a partly-lazy dependency is worse than no rule at all: a
        // named chunk is reachable from the entry, so it forces that dependency
        // eager and silently undoes the `lazy()` boundaries in `app.tsx`,
        // `sessions-route.tsx` and `date-range-filter.tsx`. The previous
        // catch-all `return "vendor"` did exactly that to @tanstack/charts,
        // react-day-picker and sonner. Everything unnamed is left to
        // Rolldown, which keeps a lazy-only module in the async chunk that needs
        // it and hoists a shared one into the entry.
        manualChunks: (id) => {
          if (!id.includes("node_modules")) {
            return;
          }
          if (EFFECT_RE.test(id)) {
            return "vendor-effect";
          }
          if (REACT_RE.test(id)) {
            return "vendor-react";
          }
          if (id.includes("node_modules/lucide-react/")) {
            return "vendor-icons";
          }
          return;
        },
      },
    },
  },
});
