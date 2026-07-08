# Peektrace — Build Plan (L1)

Spec for an AI coding agent. Companion to [`goal.md`](./goal.md) (architecture) and
[`coding-agent-inspector-ideas.md`](./coding-agent-inspector-ideas.md) (backlog).
This file = phases → steps → Definition of Done. Build in order; steps marked **∥** within a phase are independent and may run in parallel.

---

## 0. End state (acceptance)

`bunx peektrace serve` boots the Effect core, serves a Vite/React UI + Effect-RPC on `127.0.0.1`, opens the browser. The UI has three sections:

1. **Memory** — view / create / edit / delete **Claude Code** memories across **all projects** (cross-project explorer, same default as the `memory-view` skill: every memory on the machine grouped by project, with per-project drill-in), with the forensic surface (budget gauge, type donut, browse table, index↔files diff, link graph). CRUD writes back to disk atomically.
2. **Sessions** — browse **Claude** sessions, open one, see full context debug = parity with `session-report` (context budget at peak, growth timeline + dumb-zone, thinking band, loaded artifacts, full searchable history). (Codex/Pi/OpenCode are **out of scope** for the build — they appear only in the matrix as not-yet-supported.)
3. **Capabilities** — a feature × agent support matrix (Claude / Codex / Pi / OpenCode). Click a cell → why it is supported / partial / planned / unsupported (e.g. "memory edit: Claude only", "Codex sessions: planned").

`peektrace serve` is the headline. One-shot CLI commands (`sessions ls`, `memory ls`, …) exist for scripting. All disk I/O is in `core`; interfaces stay dumb.

---

## 1. Ground rules (apply to every phase)

- **Stack:** Bun runtime · Effect-TS core (`effect`, `@effect/platform`, `@effect/platform-bun`, `@effect/rpc`, `@effect/cli`, `@effect/sql-sqlite-bun`) · Effect RPC transport · Vite + React + **Effect Atom** (`@effect-atom/atom-react`) frontend. Reuse `@workspace/ui` (shadcn, React 19, Tailwind v4).
- **Do NOT touch** `apps/web` (Next.js marketing) or its oRPC `packages/api` — they stay as-is. Inspector is a **new** Vite app + a **new** Effect-RPC contract package (see deviations §2).
- **Effect conventions** (from `.agents/skills/effect-ts` + `effect-client-wrapper`): services as `Context.Tag` with a `Default` `Layer`; errors as `Data.TaggedError`; secrets via `Config.redacted`; every external/IO op wrapped with `Effect.withSpan` (OTel, per CLAUDE.md). No `try/catch`, no thrown errors across boundaries.
- **TypeScript:** infer return types; derive types from source (`typeof`, `Schema.Type<…>`); object args not positional; make impossible states unrepresentable (discriminated unions). Files ≤ 400–500 lines. JSDoc on new functions.
- **Privacy posture (inherited from the skills):** secret redaction ON by default on any rendered/exported transcript or memory body; memory/CLAUDE.md files read for size are not embedded wholesale; the core, never the model, reads bodies. Server binds loopback only.
- **Writes are agent-aware & safe:** temp-write + `rename`; compare-and-swap on mtime/hash + per-file lock; a swappable read-only `FileSystem` layer = compile-time safe-mode. Memory edit is enabled **only** where the capability registry says the agent supports it (Claude only for now).
- **Tests as real as possible** (`bun test`): real temp dirs for memory CRUD, hand-crafted redacted Claude JSONL fixtures (never ship the user's real sessions). Mock nothing that can run.
- **Per phase DoD baseline:** `bun run typecheck` + `bun run check` (ultracite) + `bun test` for touched packages pass.

---

## 2. Deviations from `goal.md` (decided)

| goal.md / original ask said | We do | Why |
|---|---|---|
| `apps/web` = Vite/React inspector | New **`apps/inspector`** (Vite). `apps/web` stays Next.js | Keep Next.js as future marketing app |
| Repurpose `packages/api` to Effect RPC | New **`packages/rpc`** (Effect RPC). `packages/api` (oRPC) untouched | Marketing app depends on oRPC; don't break it |
| Index = own SQLite + FTS5 | MVP: lazy parse + light header list; **FTS + own index deferred** to backlog | Ship the seed stories first |
| Sessions for Claude **+ Codex** | **Claude only.** Codex fully out of scope (no parsing, no SQLite) | Decided: Codex out of scope for L1 build |
| Memory per-project picker | **All projects** by default (cross-project explorer), per-project drill-in | Match the `memory-view` skill default |

Decisions are locked (see resolved list §13).

---

## 3. Target package layout

| Package | Role | Status |
|---|---|---|
| `packages/core` | Effect services: `capabilities` · `agents` (path resolvers) · `sessions` (Claude ingest+analyze) · `memory` (Claude read+CRUD, all projects) · `fs` (atomic/safe-mode) · `watch` | **new** |
| `packages/rpc` | Effect RPC contract (Schema) + handlers wiring core + client export | **new** |
| `apps/cli` | `@effect/cli` app: one-shot commands + `serve` (HTTP: RPC + static UI + open browser) | **new** |
| `apps/inspector` | Vite + React + Effect Atom UI | **new** |
| `apps/web`, `packages/api`, `packages/ui`, `packages/env`, `packages/typescript-config` | marketing + shared, **unchanged** (ui reused) | exist |
| `apps/desktop` | Electron shell | **later (out of scope)** |

---

## Phase 0 — Monorepo & Effect baseline

**Goal:** Effect toolchain installed; empty packages typecheck and lint inside Turbo.

- **0.1** Add `packages/core`, `packages/rpc`, `apps/cli`, `apps/inspector` to the workspace. Each: `package.json` (`type: module`, `@workspace/typescript-config`), `tsconfig.json` extending the right base, `src/index.ts` stub.
- **0.2** Install Effect deps at the packages that need them: `effect`, `@effect/platform`, `@effect/platform-bun`, `@effect/rpc`, `@effect/cli`, `@effect/vitest` (or `bun test`). Frontend: `@effect-atom/atom-react`, `vite`, `@vitejs/plugin-react`. (`@effect/sql-sqlite-bun` is **not** needed for MVP — own index + Codex SQLite are deferred/out of scope; add later.)
- **0.3** Turbo: add `typecheck` (exists) + `test` task; ensure new packages picked up. Biome/ultracite already global.
- **0.4 ∥** TS config for Effect: enable `exactOptionalPropertyTypes`, `strict`, `skipLibCheck` per `.agents/skills/effect-ts/core-patterns/02-tsconfig.md`. Confirm no conflict with existing bases (add a `effect-library.json` tsconfig if needed).

**DoD:** `bun install` clean · all four new packages `typecheck` + `check` green · `turbo run typecheck` includes them · no change to `apps/web` build.

---

## Phase 1 — Core: agents + capabilities + filesystem

**Goal:** Data-driven agent/feature model and the safe FS primitive everything writes through.

- **1.1 — `AgentRegistry` service.** Per-agent on-disk roots + resolvers. **Only Claude is implemented**; Codex/Pi/OpenCode have roots **declared but resolvers stubbed** (they exist for the matrix, not for parsing). Claude: `~/.claude/projects/<slug>/…` where `<slug>` = git-root path with `/` and `.` → `-`; sessions at `~/.claude/projects/**/<uuid>.jsonl`; memory at `~/.claude/projects/<slug>/memory/` (enumerate **all** project slugs). Port slug/path logic from `session-report/lib/resolve.ts` + `memory-view/lib/resolve.ts`.
- **1.2 — `CapabilityRegistry` service.** Static, typed matrix driving both the UI and write-gating:
  ```ts
  type AgentId = "claude" | "codex" | "pi" | "opencode";
  type SupportLevel = "supported" | "partial" | "planned" | "unsupported";
  type Capability = {
    id: string; group: string; title: string; description: string;
    perAgent: Record<AgentId, { level: SupportLevel; note?: string }>;
  };
  ```
  Seed the committed surfaces: `session.view`, `session.debug-context`, `memory.view`, `memory.crud`, plus backlog rows (`mcp.dashboard`, `skills.browser`, `file-history.diff`, …) as `planned`/`unsupported`. Truth for now: **claude only is built** — `memory.view`/`memory.crud`/`session.view`/`session.debug-context` = claude `supported`; codex/pi/opencode = `planned` for all of these (Codex is out of scope for the build but still a column so the matrix shows the gap).
- **1.3 — `FileSystem` access + safe-mode.** Thin wrapper over `@effect/platform` FS exposing `readText`, `stat`, and `atomicWrite` (temp + `rename`) + `withFileLock` + compare-and-swap (`expectedMtime`/hash). Provide two layers: `FsLive` (RW) and `FsReadOnly` (mutations fail at the type/layer level). Tagged errors: `FileChangedError`, `WriteDeniedError`, `PathOutsideRootError` (reject paths escaping the agent roots).

**DoD:** unit tests: slug resolver round-trips known paths · capability registry exhaustive over `AgentId` (compile-time `Record`) · `atomicWrite` to a temp dir is atomic and CAS rejects a stale mtime · `FsReadOnly` makes `atomicWrite` unavailable (type error in a `// @ts-expect-error` test) · path-escape rejected.

---

## Phase 2 — Core: session ingest + analysis (port `session-report`, Claude only)

**Goal:** Parse Claude transcripts and reproduce the context-budget math headlessly. Source of truth to port: `session-report/scripts/lib/{parse,analyze,tokens,redact,types}.ts`. (`lib/codex.ts` is **not** ported — Codex out of scope.)

- **2.1 — Schemas.** `effect/Schema` for Claude transcript line types (`user|assistant|system|attachment|mode|permission-mode|file-history-snapshot|ai-title|last-prompt`, carrying `cwd|gitBranch|parentUuid|isSidechain|requestId|timestamp|version|usage`). Derive TS types from schemas.
- **2.2 — `SessionsService.list`.** Enumerate Claude `projects/**/*.jsonl`. Return lightweight headers: id, project/cwd, gitBranch, model, started/updated, message count, size. Lazy — do **not** fully parse bodies here. Use Effect `Stream` for backpressured directory + line reads.
- **2.3 — `SessionsService.parse(id)`.** Full parse of one transcript → normalized event stream, including subagent/sidechain transcripts (`isSidechain`, `parentUuid`) unless skipped.
- **2.4 — `SessionsService.analyze(id)`.** Reproduce `session-report` forensics: per-turn exact context from `usage` (`input + cache_read + cache_creation`); peak context vs window (default 1M for Claude); budget partition (system+tools, listings, CLAUDE.md/memory size-from-disk, opened files, prompts, tool results, assistant text, **thinking** = `output_tokens` − visible, unattributed residual); growth timeline points + dumb-zone crossing (default 0.40) + compaction cliffs; biggest items; loaded artifacts (sizes only). Return as serializable analysis object (no HTML — UI renders).
- **2.5 — Redaction.** Port `lib/redact.ts`; apply to any transcript text leaving core. Default on; expose `redact: boolean` per request.

**DoD:** fixture-driven `bun test`: a hand-crafted Claude JSONL fixture yields expected peak/budget/thinking numbers (assert against committed golden) · subagent transcript folds into parent · redaction masks a planted fake key · `list` over a fixtures dir returns correct headers without parsing bodies.

---

## Phase 3 — Core: memory read + CRUD (port `memory-view`, then extend)

**Goal:** Full Claude memory model + safe CRUD. Source to port: `memory-view/scripts/lib/{parse,frontmatter,reindex,graph,audit,resolve,types}.ts`.

- **3.1 — Read model.** `MemoryService.listProjects()` (every project on the machine with a non-empty `memory/`), `getAllVaults()` (the cross-project default — all memories grouped by project + a project overview), `getVault(project)` → entries (`name`, `description`, `type`, body, size, mtime, inIndex, links), `MEMORY.md` index parse + budget (200 lines / 25 KB cliff, mark below-fold "INVISIBLE TO CLAUDE"), index↔files diff (orphans/dangling), `[[wikilink]]` graph, type counts. Frontmatter parse/serialize round-trips losslessly. Mirror the `memory-view` skill's default (all-projects) and named (single-project) scopes.
- **3.2 — Create.** `create({project, name, description, type, body})`: validate `name` kebab/unique + `type ∈ user|feedback|project|reference`; scaffold frontmatter; `atomicWrite` `memory/<name>.md`; insert `MEMORY.md` index pointer line (`- [Title](file.md) — hook`). Returns new entry.
- **3.3 — Edit.** `update({project, name, frontmatter?, body?})`: CAS on file mtime; re-serialize; re-validate budget + links; keep index line in sync if title/description changed.
- **3.4 — Delete.** `delete({project, name})`: remove file + its `MEMORY.md` line; report now-dangling references it leaves behind.
- **3.5 — Gating.** All mutations route through Phase 1 FS + check `CapabilityRegistry`: only `claude` `memory.crud=supported` permits writes; any non-Claude agent is rejected with a typed `CapabilityUnsupportedError`. (Codex/Pi/OpenCode memory is out of scope — no read, no write.)

**DoD:** `bun test` against a real temp vault: create → file on disk + index line present + budget recomputed · edit body → CAS blocks a stale write, succeeds on fresh · delete → file + index line gone, dangling links reported · frontmatter round-trip is byte-stable for unchanged fields · attempting a codex write returns `CapabilityUnsupportedError`.

---

## Phase 4 — RPC contract + handlers (`packages/rpc`)

**Goal:** One typed Effect-RPC surface over the core; client export for the UI.

- **4.1 — Contract.** `@effect/rpc` `RpcGroup` with `Schema` requests/responses:
  - `capabilities.list`
  - `sessions.list` (filter: project) · `sessions.get` · `sessions.analyze` (args: id, window?, dumbZone?, redact?)
  - `memory.allVaults` · `memory.projects` · `memory.vault(project)` · `memory.create` · `memory.update` · `memory.delete`
  All payload types derived from core schemas (no hand-duplication). Errors surfaced as typed RPC failures (`FileChangedError`, `CapabilityUnsupportedError`, …).
- **4.2 — Handlers.** Implement the group against core services; compose required layers. Keep handlers thin (no logic).
- **4.3 — Client.** Export a typed RPC client factory (`createPeektraceClient(baseUrl)`) for in-process and HTTP transports. End-to-end types: disk → core → UI.

**DoD:** an in-process `bun test` drives every procedure through the real handlers + core against fixtures (no HTTP) and gets typed results · a forced `memory.update` CAS conflict returns the typed error over RPC · client types compile against the contract with zero `any`.

---

## Phase 5 — CLI app (`apps/cli`) + `serve`

**Goal:** `peektrace` binary: scriptable commands + the browser server.

- **5.1 — `@effect/cli` skeleton.** `peektrace` with subcommands. Two execution modes: **in-process** (provide core layers directly) and `--remote <url>` (RPC HTTP client). Global flags: `--read-only` (swap `FsReadOnly`), `--json`.
- **5.2 — One-shot commands.** `sessions ls [--agent] [--project]`, `sessions analyze <id>`, `memory ls [project]`, `memory show <project> <name>`, `memory rm <project> <name>`. Render tables to stdout; `--json` emits raw.
- **5.3 — `serve` command.** `@effect/platform-bun` `BunHttpServer`: mount RPC handler at `/rpc`; serve the built inspector static assets (from `apps/inspector/dist`) at `/`; bind `127.0.0.1:<port>` (default + auto-pick if busy); open browser (`--open`, default true); `--no-open`, `--port`. Provide all core layers once at server boot; integrate `watch` (Phase 9) for live updates.

**DoD:** `peektrace sessions ls` lists real local sessions · `peektrace memory ls` lists projects with memory · `peektrace serve --no-open` starts, `curl /rpc` returns a valid RPC response, `/` serves `index.html` · `--read-only` makes `memory rm` fail with a clear message · `--remote <url>` hits a running server.

---

## Phase 6 — Inspector shell (`apps/inspector`)

**Goal:** Vite/React app, Effect-Atom state, RPC client wired, dev↔serve parity.

- **6.1 — Vite + React 19 + Tailwind v4** importing `@workspace/ui` (shadcn). App shell: left nav (Memory · Sessions · Capabilities), routing, dark mode (`next-themes` already in ui).
- **6.2 — Effect Atom data layer.** Atoms wrapping the RPC client; loading/error/success as a discriminated union (no flag bags). One `client` atom configurable by base URL.
- **6.3 — Dev vs prod transport.** Dev: Vite dev server + proxy `/rpc` → running `peektrace serve` (or a `dev:server` script). Prod: same-origin (served by `serve`). Document both in `apps/inspector/README.md`.

**DoD:** `bun dev` (inspector) renders the shell, reaches `/rpc`, shows live `capabilities.list` data · production build (`vite build`) emits `dist/` that `peektrace serve` serves successfully.

---

## Phase 7 — Memory UI (CRUD) — *first vertical slice; demo target*

**Goal:** Full Claude memory management surface.

- **7.1 ∥** Default = **all-projects explorer** (`memory.allVaults`): project overview on top, then every memory grouped by project (accordion); search + type-filter span all projects. Plus single-project drill-in (`memory.projects` / `memory.vault`).
- **7.2 ∥** `memory-view` parity surface: budget gauge (200-line/25 KB, below-fold greyed "INVISIBLE TO CLAUDE"), type donut, sortable/filterable/searchable browse table (title·type·description·size·modified·in-index·links), row expand → full detail + clickable `[[links]]`, index↔files diff panel, link graph.
- **7.3** Create: form (name/description/type/body) → `memory.create`, optimistic, re-validate gauge.
- **7.4** Edit: inline/body+frontmatter editor → `memory.update`; surface CAS conflict (file changed under you) with reload-or-overwrite choice.
- **7.5** Delete: confirm → `memory.delete`; show dangling-link warnings returned by core.
- **7.6** Capability-aware: non-Claude agents show the matrix verdict + disabled editing (no write affordance).

**DoD:** in a throwaway temp project a user can create, edit, delete a memory end-to-end through the UI, changes land on disk (verified by re-reading), gauge/diff/graph update live, and a concurrent external edit triggers the CAS conflict UI.

---

## Phase 8 — Session viewer UI (debug) — *second vertical slice*

**Goal:** Browse + full context debug, parity with `session-report`, for **Claude**.

- **8.1 ∥** Session list: filter by project/branch/model/date, search by title; lazy from `sessions.list`.
- **8.2** Session debug view from `sessions.analyze`: verdict header + peak gauge (Healthy/Degrading/Rotting); **budget-at-peak** stacked bar + table (incl. the thinking band + unattributed); **growth timeline** (SVG area, dumb-zone band, compaction cliffs, peak marker, first dumb-zone crossing); loaded artifacts (sizes, "trim me"); biggest items.
- **8.3** Full collapsible history: every event (tool calls + results + attachments), search/filter, dumb-zone divider inline, sidechain/subagent drill-down. Render redacted by default; "review before sharing" banner.

**DoD:** opening a real local Claude session reproduces the same peak/budget/thinking figures the `session-report` HTML would (spot-check one session against the skill output) · history search + sidechain expand work · redaction visibly on by default.

---

## Phase 9 — Capabilities matrix UI + live watch

**Goal:** The feature-support table, plus filesystem-driven freshness.

- **9.1** Matrix from `capabilities.list`: rows = features (grouped), columns = Claude/Codex/Pi/OpenCode, cells colored by `SupportLevel`. Click a cell → detail drawer with `note` (e.g. "memory edit: Claude markdown only; Codex is auto-generated SQLite — read-first"). Legend.
- **9.2 — `WatchService`** (`@effect/platform` file watcher) on the agent roots; push invalidations over RPC (stream/poll) so Memory + Sessions auto-refresh when an agent writes. CAS already protects writes; this keeps reads fresh.

**DoD:** matrix renders all seeded capabilities with correct per-agent levels and working detail drawers · editing a memory file outside the app refreshes the Memory list within the watch interval · adding a new session file shows up in the list without manual reload.

---

## Phase 10 — Hardening, packaging, docs

**Goal:** Ship-quality seed.

- **10.1** `peektrace serve` packaged path: `vite build` → `serve` ships the static UI; document `bunx peektrace serve`. Optional `bun build --compile` single-binary spike (note only).
- **10.2** Error/empty states everywhere: no projects with memory, no sessions, unreadable/locked files, malformed transcripts (defensive parsing — undocumented formats may drift; skip-and-warn, never crash).
- **10.3** OTel: confirm spans on every core IO op; a `--otel` flag or env wiring an exporter (no-op default).
- **10.4** Docs: root `README` section + `apps/cli/README` (commands, modes) + `apps/inspector/README` (dev/prod). Update `goal.md` package table footnote pointing to the `packages/rpc` / `apps/inspector` deviation.
- **10.5** Final acceptance: walk the §0 end state on real local data.

**DoD:** §0 acceptance passes on the author's machine · `apps/web` (Next.js) still builds and runs untouched · typecheck + ultracite + tests green across the repo.

---

## 11. Suggested build order for the agent

Vertical-slice first so there's a demo early:
**P0 → P1 → P3 (memory core) → P4 (memory RPC subset) → P5 (`serve` + memory cmds) → P6 → P7 (memory UI demo)**, then
**P2 (session core) → P4 (session RPC) → P8 (session UI)**, then **P9 (matrix + watch) → P10**.
Phase 1 capability registry is a dependency of P7/P8/P9 — do it early. Within phases, **∥** steps fan out.

## 12. Testing & quality strategy

- `bun test` per package; fixtures committed under `packages/core/test/fixtures` (synthetic, redacted Claude JSONL + a temp memory vault spanning ≥2 projects). Golden files for analysis math.
- Real temp dirs for all write tests. No mocked FS.
- Every phase gate: `bun run typecheck` + `bun run check` + touched-package tests.
- Keep files ≤ 500 lines; split renderers/parsers by concern as the source skills do.

## 13. Resolved decisions (locked)

1. **Memory scope → all projects.** Default = cross-project explorer over every `~/.claude/projects/<slug>/memory/` on the machine (same as the `memory-view` skill), with single-project drill-in. Not a single-project picker.
2. **RPC → new package.** Add `packages/rpc` (Effect RPC); leave oRPC `packages/api` + `apps/web` (Next.js marketing) untouched.
3. **Inspector → `apps/inspector`** (new Vite app).
4. **Sessions → lazy.** Lazy headers + parse-on-open. Own SQLite/FTS5 index deferred to backlog.
5. **Codex → fully out of scope.** No Codex parsing, no SQLite, no memory. Codex (+ Pi, OpenCode) appear only as not-yet-supported columns in the capability matrix.

### Still open (non-blocking — sensible defaults assumed)

- **`--remote` / VPS:** contract supports it; keep the light CLI `--remote` flag now, no remote-specific UI work until the local slice lands. (Assumed.)
</content>
</invoke>
