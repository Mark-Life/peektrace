# Peephole — Goal & Architecture

Companion to [`coding-agent-inspector-ideas.md`](./coding-agent-inspector-ideas.md). That file is the full feature dump; this file pins down **what we build first** and **how it's structured**.

---

## 1. General goal

A **local-first inspector and editor for coding-agent state** on a single machine — Claude Code and Codex now, Pi / OpenCode later. One pane over the stuff agents scatter across `~/.claude`, `~/.codex`, etc.: sessions, memory, skills, MCP, rules, file-history.

Scope is **L1 = local viewer/editor**. No cloud sync (L2), no code-folder sync (L3). Single machine, your own data.

The bet, grounded in the ideas dump: nobody surfaces this state well, and it's fragmented in ways only a cross-agent tool can fix —

- **Memory isn't one thing** — Claude = curated markdown, Codex = auto-generated SQLite, Pi = `AGENTS.md`. An editor has to be agent-aware.
- **MCP config is scattered** across 3+ formats/locations.
- **Skills drift** across agents (`session-report` already duplicated in 3).
- **`file-history/` = rewind data nobody exposes.**

---

## 2. Starting point — two skills become the first product stories

Two existing, proven skills are the seed. Both are **user-invoked, deterministic, zero-dep TypeScript that emits a self-contained, secret-redacted, human-facing HTML artifact** — local and private, never fed back to an LLM. Peephole keeps that privacy posture but turns the **one-shot HTML into a live, persistent, interactive surface** in the app.

### Story 1 — Session inspector (from `session-report`)

Source skill: [`agent-skills/skills/session-report/SKILL.md`](/Users/andrey/Code/personal/agent-skills/skills/session-report/SKILL.md)

What it already does → what Peephole surfaces live:

- **Context budget at peak** — where every token goes: system+tools, listings, CLAUDE.md/memory, opened files, prompts, tool results, assistant text, **thinking** (the hidden giant), unattributed overhead.
- **Context-growth timeline** — per-turn context size, the dumb-zone band (~40% context-rot cutoff), compaction cliffs, peak marker.
- **Loaded artifacts + biggest items + full searchable history.**
- Works for Claude (`~/.claude/projects/**/<uuid>.jsonl`) and Codex (`~/.codex/sessions/.../rollout-*.jsonl`, which records its real window).

In the app this becomes a **persistent, browsable session viewer** instead of a regenerated file — same forensic math, but multi-session, searchable, always current.

### Story 2 — Memory **viewer → editor** (from `memory-view`)

Source skill: [`agent-skills/skills/memory-view/SKILL.md`](/Users/andrey/Code/personal/agent-skills/skills/memory-view/SKILL.md)

The source skill is **read-only by design** (the model never edits; the script only renders). Peephole's deliberate step beyond it: **the memory surface is also an editor.**

Keep from the skill:

- **`MEMORY.md` budget gauge** — fill vs the 200-line / 25 KB cliff; below-the-fold entries marked **"INVISIBLE TO CLAUDE."**
- **Type donut** (user/feedback/project/reference), browse table, **index-vs-files diff** (orphans vs dangling), **link graph** of `[[wikilinks]]`.

Add — full **CRUD**:

- **Create** a new memory (frontmatter scaffolded: `name`, `description`, `type`) + auto-insert its `MEMORY.md` index pointer.
- **Edit** body + frontmatter, with live re-validation of the budget gauge and links.
- **Delete** a memory and clean up its index line / dangling references.
- **Atomic, agent-aware writes** (see §4): Claude memory is markdown → safe to edit. Codex memory is auto-generated SQLite → **read-first**; editing gated behind explicit, clearly-flagged opt-in.

These two stories are the **first slice of functionality**. Everything else — unified MCP dashboard, skill drift detector, checkpoint diff/restore, prompt library, usage analytics, secret scanner, cross-agent drift — is the backlog in [`coding-agent-inspector-ideas.md`](./coding-agent-inspector-ideas.md), pulled in after the seed lands.

---

## 3. Architecture — server-first core, thin interfaces

One principle: **all logic lives in the core; every interface is dumb.** The core is a headless server (a set of Effect layers). Interfaces (CLI now, Electron later, web/VPS for free) only parse input and render output.

```
                 ┌──────────── interfaces (thin) ────────────┐
   CLI  (now) ───┤                                            │
   Electron(later)┤   web UI (Vite/React + Effect Atom)        │
   browser/VPS ──┘                                            │
                          │  Effect RPC (typed contract)
                 ┌────────▼──────────── core (Bun) ───────────┐
                 │  ingest   agent-aware parsers              │
                 │           (Claude JSONL · Codex SQLite ·   │
                 │            Pi/OpenCode later)              │
                 │  index    own SQLite + FTS5 (search,       │
                 │            analytics, drift)              │
                 │  edit     atomic, agent-aware writes       │
                 │  watch    FS watcher → live updates        │
                 └────────────────────────────────────────────┘
                          reads/writes  ~/.claude · ~/.codex · …
```

### Stack

- **Runtime:** Bun. **Core:** Effect-TS (`@effect/platform-bun` for FS / HTTP / watch, `@effect/sql-sqlite` for our index + reading Codex, Effect Stream for backpressured JSONL parsing).
- **Transport:** Effect RPC — one typed contract, end-to-end types from disk → core → UI, no hand-duplicated shapes.
- **Frontend:** Vite + React, **Effect Atom** for state.
- **Monorepo:** Bun workspaces + Turbo + Biome/Ultracite (already in place).

### Why server-first (not desktop-native)

- The work is **I/O-bound** (reading JSONL, querying SQLite, watching files) — not compute. Rust/Tauri would buy marginal I/O gains while costing us Effect and end-to-end types. Rejected for L1.
- A server core runs three ways from one codebase: **CLI** (now), **Electron** shell (later), and **headless on a VPS behind a port** (debug an agent on a remote box from a browser) — the last is impossible with a desktop-only build.

### Package layout (maps onto existing scaffold)

| Package | Role | Status |
|---|---|---|
| `packages/core` | domain + Effect services: `ingest` · `index` · `edit` · `watch` | new |
| `packages/api`  | Effect RPC contract (shared types) | exists → repurpose [^deviation] |
| `apps/web`      | Vite/React + Effect Atom UI | exists [^deviation] |
| `apps/cli`      | technical interface (boots core; one-shot commands) | **new, first** |
| `apps/desktop`  | Electron shell wrapping web + core | **later** |
| `packages/ui`, `packages/env`, `packages/typescript-config` | shared UI / env / tsconfig | exist |

[^deviation]: **Built differently than this table.** The RPC contract lives in a
**new `packages/rpc`** (Effect RPC) — `packages/api` (oRPC) is left untouched —
and the inspector UI is a **new `apps/inspector`** (Vite) rather than repurposing
`apps/web`, which stays Next.js marketing. Both `apps/web` and `packages/api` are
intentionally not modified so the marketing app keeps building. See the locked
deviations in [`plan.md` §2](./plan.md#2-deviations-from-goalmd-decided).

### File edits — how writes work safely

Browser/Electron/CLI **never touch disk**; the **core does**, behind one typed RPC mutation. This boundary is identical local or remote.

- **Atomic writes:** write temp + `rename`, with Effect giving rollback-on-error.
- **Race safety:** core also watches the same files the agent writes. Compare-and-swap (read mtime/hash, write only if unchanged) + per-file lock prevents clobbering a concurrent agent write.
- **Agent-aware:** `editMemory` hides the difference between Claude markdown (safe) and Codex SQLite (risky, opt-in). The UI doesn't care.
- **Safe-mode:** a read-only `FileSystem` layer swapped in at the type level — a compile-time guarantee, not a runtime `if`.

### Rollout

1. **CLI first.** Boots core, runs the two seed stories (session inspector, memory viewer+editor) as commands + a `serve` mode that opens the browser UI at `127.0.0.1`. Two CLI modes: *in-process* (imports core layers directly — one-shot/scripting) and `--remote <url>` (RPC client → a running/remote server).
2. **Electron later.** Non-technical front door: double-click → boots core as a child process → loads the same web UI. No new logic — just packaging.
3. **VPS** falls out for free: same core, bound to a port, browser over SSH tunnel.

The discipline that makes all three cheap: **keep interfaces dumb.** No parsing, no FS logic outside `core`.
