# Bundle size: the inspector's first paint

**Date:** 2026-08-09 · **Measured on:** `apps/inspector` production build (Vite 8 / Rolldown), `apps/web` Next 16 build

The inspector shipped as one eager graph: every dependency any route could
reach was fetched before the first pixel. This pass split it at the boundaries
that never paint, deleted one dependency the transport never uses, and stopped
Tailwind generating classes for components nothing imports.

| | Before | After | Δ |
|---|---:|---:|---:|
| **First paint** (entry + preloads + CSS) | 1159.5 KiB raw · 332.6 KiB gzip | 801.4 KiB raw · **233.4 KiB gzip** | **−30.9% raw · −29.8% gzip** |
| Whole `dist/` | 1159.5 KiB · 332.6 KiB | 1092.7 KiB · 326.6 KiB | −5.8% · −1.8% |
| `apps/web` client CSS | 162.0 KiB | 113.8 KiB | −29.7% |

Before, those two rows were the same number: nothing was split, so the whole
`dist/` *was* the first paint. Most of the win is bytes deferred rather than
deleted — 66.8 KiB raw is genuinely gone (msgpackr + CSS), the other ~291 KiB
now arrives when something actually renders it.

## Reproducing

```sh
bun run --filter=inspector build     # per-chunk raw + gzip, printed by Vite
bun run --filter=web build && find apps/web/.next/static -name '*.css' -exec ls -l {} \;
```

Sizes above are `gzip -9` over every file in `apps/inspector/dist`, with the
first-paint set taken from the `<script>` and `<link rel=modulepreload>` tags
Vite emits into `index.html` — i.e. what the browser fetches before rendering,
not what the directory weighs.

## What moved, and why it hadn't already

### The catch-all chunk rule was the load-bearing bug

`vite.config.ts` ended its `manualChunks` with `return "vendor"` for anything
under `node_modules`. A named chunk is reachable from the entry, so that one
line pinned **every** third-party dependency to the first paint. Any `lazy()`
boundary added above it would have been silently undone: the component would
split, its dependencies would not.

That is why splitting had to come with the chunking change, and why only trees
the first paint already pulls in whole (`effect`, `react-dom`, `lucide-react`)
are named now. Everything else is left to Rolldown, which keeps a lazy-only
module in the async chunk that needs it and hoists a shared one into the entry.

### The four boundaries

| Boundary | Deferred | Trigger |
|---|---:|---|
| `memory` / `capabilities` / `settings` routes | 134 KiB (`@tanstack/charts` 86.5, `d3-*` 5.7, the memory dialogs) | nav click |
| `SessionDetail` inside the sessions route | 44 KiB (`@tanstack/highlight` 12.5, the forensic charts, the transcript) | picking a session |
| `Calendar` inside the date filter's popover | 72 KiB (`react-day-picker` 39, `date-fns` 22, `@date-fns/tz` 4) | opening the date facet |
| `Toaster` | 33 KiB (`sonner`) | mounts on its own after the entry runs |

`sessions` stays eager: it is what an empty hash resolves to, so splitting it
would only add a round trip. The rail — the list, its filters, the empty detail
pane — is all that now paints first.

### msgpackr, deleted

`@effect/rpc`'s `RpcSerialization` serves NDJSON and MessagePack from one
module and pulls msgpackr in with a static `import * as Msgpackr`. `RpcClient`
imports that module, so 27 KiB of MessagePack shipped no matter which
serialization the app built — and msgpackr is side-effecting CJS, so tree
shaking will not drop it. `src/stubs/msgpackr.ts` is aliased in its place and
throws if MessagePack is ever actually constructed; the inspector's transport
is NDJSON (`src/lib/client.ts`). Rolldown then shakes the stub out too.

This is a build-time alias for the browser only. The CLI's server keeps the
real package.

### 28 shadcn components nothing imported

`packages/ui` carried 56 components; 28 had no importer anywhere in the repo,
`demo.tsx` included. They cost nothing in JS — unimported modules never enter a
bundle — but `globals.css` scans `packages/ui/src/**` for class names, so every
one of them contributed utilities to **both** apps' CSS. Deleting them took the
inspector's stylesheet from 162.8 KiB to 115.0 KiB (24.6 → 18.5 gzip) and the
web app's from 162.0 to 113.8 KiB.

`shadcn add <name>` brings any of them back.

Two dead `@source` globs went with them: `../../../apps/**` and
`../../../components/**` resolve against `packages/ui/src/styles`, so they
pointed at `packages/apps` and `packages/components`, neither of which exists.
Removing them produced a byte-identical stylesheet, which is the proof they were
dead. The consuming app's own files come from Tailwind's automatic detection.

Eight npm dependencies were only ever reached through the deleted components and
are gone from `packages/ui`: `@base-ui/react`, `cmdk`, `date-fns`,
`embla-carousel-react`, `input-otp`, `react-resizable-panels`, `vaul`, `zod`.

## What is left, in order of size

| Eager chunk | gzip | Verdict |
|---|---:|---|
| `vendor-effect` | 101.6 KiB | Load-bearing. `Schema` + `SchemaAST` + `ParseResult` (64 KiB minified) decode every RPC response, and the sessions list is the first thing fetched. |
| `vendor-react` | 56.1 KiB | `react-dom`. Fixed cost. |
| `index` (app + Radix) | 36.8 KiB | Application code. |
| `index.css` | 18.5 KiB | Now mostly components both apps genuinely use. |
| `tailwind-merge` | 10.2 KiB | Runtime cost of `cn()`. Real, but replacing it is a correctness risk for a class-conflict resolver used everywhere. |

Two things would move the needle further, neither cheap:

1. **Per-app CSS partitioning.** The shared `globals.css` scans all of
   `packages/ui/src`, so the inspector still generates utilities for components
   only `apps/web` renders (and vice versa). Fixing it means each app declaring
   its own `@source` set — a hand-maintained list whose failure mode is a
   silently missing style, which is worse than the bytes.
2. **Trimming Effect's schema surface.** Nothing short of changing how the RPC
   contract validates would help, and that is a transport decision, not a bundle
   one.

## Unverified

- **No browser.** This environment has none. The lazy boundaries were exercised
  under happy-dom instead — shell, all three lazy routes, the `Toaster` (fired a
  toast to force it to paint) and the `Calendar` inside its Radix portal all
  mounted and rendered. That harness needed a global DOM registration which
  breaks the 28 non-DOM tests sharing the process, so it was not kept; the repo
  has no DOM test setup to hang it on.
- **The CLI binary was not built here.** `bun run --filter=peektrace build`
  fails on linux-arm64 for want of msgpackr's optional `msgpackr-extract`
  native package. It fails identically on `main`, so it is unrelated to this
  change, but it does mean the embedded-asset path was only checked by serving
  `dist/` from disk (`peektrace serve`, all 20 assets 200).
- **`turbo.json` does not cache the inspector build.** Its `build.outputs` lists
  only `.next/**`, so `inspector#build` warns "no output files found" and
  re-runs every time. Pre-existing; not touched here.
