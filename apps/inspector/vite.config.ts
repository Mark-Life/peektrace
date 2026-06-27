import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/**
 * Inspector Vite config.
 *
 * - Tailwind v4 via `@tailwindcss/vite` (styles come from
 *   `@workspace/ui/globals.css`, imported in `main.tsx`).
 * - Dev transport: proxy `/rpc` to a running `peephole serve` so the dev server
 *   and the production (same-origin) build hit an identical RPC path. Override
 *   the target with `PEEPHOLE_RPC_TARGET` (default `http://127.0.0.1:4321`).
 * - Prod transport: none needed — `peephole serve` hosts `dist/` and `/rpc`
 *   on the same origin, so the default base URL `""` resolves `/rpc` directly.
 */
const RPC_TARGET = process.env.PEEPHOLE_RPC_TARGET ?? "http://127.0.0.1:4321";

const EFFECT_RE = /node_modules\/(effect|@effect|@effect-atom)\//;
const REACT_RE = /node_modules\/(react|react-dom|scheduler)\//;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      "/rpc": {
        target: RPC_TARGET,
        changeOrigin: true,
      },
    },
  },
  build: {
    rollupOptions: {
      output: {
        // Split the two heavy, rarely-changing vendor trees out of the app
        // chunk so neither the >500KB single-chunk warning fires nor a small UI
        // edit busts the big Effect/React caches.
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
          if (id.includes("node_modules/@radix-ui/")) {
            return "vendor-radix";
          }
          if (id.includes("node_modules/lucide-react/")) {
            return "vendor-icons";
          }
          // Everything else third-party (tailwind-merge, recharts, etc.).
          return "vendor";
        },
      },
    },
  },
});
