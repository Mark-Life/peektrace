@.agents/skills/quality-code/SKILL.md

When making decisions on new stack, or libs to add, read stack options from the template author at `.docs/stack.md`.

## Typecheck

Typecheck with `bun run typecheck` (per-package `tsc --noEmit`). Never run `tsc -b` or bare `tsc` — build/emit mode writes `.js`/`.d.ts`/`.d.ts.map` next to sources and pollutes the tree; stale emitted `*.test.js` then get double-run by `bun test`. The base tsconfig sets `noEmit`, but build mode (`-b`) bypasses it. This repo never builds via tsc (Bun runs TS directly; the inspector builds with Vite; web with Next).
