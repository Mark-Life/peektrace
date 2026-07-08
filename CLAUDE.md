## Code Quality
When writing or reviewing TypeScript/full-stack code, follow the `quality-code` skill (`.agents/skills/quality-code/SKILL.md`). It loads on demand — invoke it for the full standards.


## Typecheck
Typecheck with `bun run typecheck` (per-package `tsc --noEmit`). Never run `tsc -b` or bare `tsc` — build/emit mode writes `.js`/`.d.ts`/`.d.ts.map` next to sources and pollutes the tree; stale emitted `*.test.js` then get double-run by `bun test`. The base tsconfig sets `noEmit`, but build mode (`-b`) bypasses it. This repo never builds via tsc (Bun runs TS directly; the inspector builds with Vite; web with Next).

## Local Effect Source
Two Effect checkouts are cloned locally for reference (we're mid-transition, so both matter):

- **v3** (current stable): `~/.local/share/effect-solutions/effect` — `effect@3.21.0`, the main `Effect-TS/effect` repo.
- **v4** (smol / next): `~/.local/share/effect-solutions/effect-smol` — `effect@4.0.0-beta.x`, the `Effect-TS/effect-smol` repo.

Use these to explore APIs, find usage examples, and understand implementation details when the documentation isn't enough. Check the version that matches the code you're touching; when in doubt, consult both.