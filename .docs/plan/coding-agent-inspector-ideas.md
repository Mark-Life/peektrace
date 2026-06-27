# Coding Agent Inspector — L1 App Idea Dump

Scope: **L1 = local viewer/editor** for coding-agent state on a single machine.
No sync (that's L2), no code-folder sync (that's L3).
Committed surfaces so far: **session viewer** (Claude, Codex; future Pi, OpenCode) + **memory view/edit** (Claude).

This file is a *complete* dump of possibilities to pick from later. Nothing is prioritized as final.

---

## 1. Raw material — what's actually on disk

Grounded from inspecting this machine (2026-06-26).

### Claude (`~/.claude`) — JSONL + JSON based

| Path | What it is | Feature potential |
|------|-----------|-------------------|
| `projects/**/<uuid>.jsonl` | 1,402 sessions, 295MB. Line types: `user`, `assistant`, `system`, `attachment`, `mode`, `permission-mode`, `file-history-snapshot`, `ai-title`, `last-prompt`. Carries `cwd`, `gitBranch`, `parentUuid`, `isSidechain`, `requestId`, `timestamp`, `version` | Session viewer, tree/fork view, search, export |
| `~/.claude.json` | Mega-config: `mcpServers`, `projects` (per-project trust/allowed tools), `toolUsage`, `skillUsage`, `pluginUsage`, `tipsHistory`, OAuth account, caches | MCP dashboard, usage leaderboards, per-project config |
| `CLAUDE.md` (user) + project `CLAUDE.md` / `.claude/CLAUDE.md` / `.claude.local.md` | Memory/instructions, markdown, layered | Rules resolver, memory edit |
| `file-history/<session>/<hash>@vN` | Versioned file snapshots per session (28MB) = checkpoint/rewind data | Checkpoint diff + restore viewer |
| `tasks/<uuid>/` | Background agent state (`.lock`, `.highwatermark`) | Background tasks viewer |
| `plans/*.md` | Saved plan-mode plans | Plans browser |
| `history.jsonl` | 3,551 past prompts | Prompt library / search / reuse |
| `usage-data/` | Ships its own `report.html` + `facets/` + `session-meta/` | Usage dashboard (beat the built-in) |
| `stats-cache.json` | `dailyActivity`, `dailyModelTokens`, `modelUsage`, `hourCounts`, `longestSession`, `firstSessionDate`, `totalSpeculationTimeSavedMs` | Analytics, heatmaps |
| `skills/` | `ai-sdk`, `find-skills`, `session-report`, `vercel-react-best-practices`, `web-design-guidelines` | Skills browser/editor |
| `agents/` | Subagent definitions | Subagent browser/editor |
| `commands/` | Custom slash commands | Commands browser/editor |
| `plugins/` | `installed_plugins.json`, `known_marketplaces.json`, `blocklist.json`, marketplace caches | Plugin/marketplace browser |
| `settings.json` / `settings.local.json` | Permissions, hooks, env, model | Permissions + hooks viewers |
| `shell-snapshots/` | Captured shell env per session | Debug/repro context |
| `ide/`, `paste-cache/`, `telemetry/`, `backups/`, `mcp-needs-auth-cache.json` | Misc state | Auth status, backups restore |

### Codex (`~/.codex`) — SQLite based (fundamentally different)

| Path | What it is | Feature potential |
|------|-----------|-------------------|
| `memories_1.sqlite` (`stage1_outputs`) | **Auto-generated memory pipeline**: `raw_memory`, `rollout_summary`, `usage_count`, `last_usage`, `selected_for_phase2`. Not hand-curated | Memory viewer (read-first; editing is risky) |
| `state_5.sqlite` | `threads`, `agent_jobs`, `agent_job_items`, `thread_spawn_edges`, `thread_dynamic_tools`, `remote_control_enrollments` | Thread graph, job/agent viewer |
| `goals_1.sqlite` (`thread_goals`) | Per-thread goals | Goals viewer |
| `logs_2.sqlite` | Logs (~7MB) | Log explorer |
| `sessions/YYYY/MM/DD/rollout-*.jsonl` | Date-foldered session rollouts | Session viewer |
| `session_index.jsonl` | Session index | Fast session list |
| `external_agent_session_imports.json` | **Codex already imports other agents' sessions** | Precedent for cross-agent import |
| `config.toml` | `model`, `mcp_servers`, `marketplaces`, `plugins` | MCP + config viewer |
| `AGENTS.md` | Instructions (note: not CLAUDE.md) | Rules resolver |
| `skills/`, `plugins/` | Same shape as Claude; `session-report` duplicated here | Skill drift detector |
| `auth.json` | Credentials | Secret scanner target |

### Pi (`~/.pi/agent`) — future

`AGENTS.md`, `settings.json`, `models.json`, `sessions/`, `skills/` (incl. duplicated `session-report`, `ai-sdk`), `auth.json`, `.env.local`, `bin/` (bundled tools like `fd`).

### OpenCode (`~/.config/opencode`) — future

`opencode.jsonc` (MCP config, e.g. react-grab-mcp), `skills/`.

### Three structural facts that should shape the product

1. **Memory is not one thing.** Claude = hand-curated markdown. Codex = auto-generated SQLite pipeline (`stage1_outputs`). Pi = `AGENTS.md`. "Edit memory" means something different per agent — the editor must be agent-aware.
2. **Skills already drift across agents.** `session-report` exists in Claude, Codex, and Pi independently. Same for `ai-sdk`. There is no single source of truth today.
3. **MCP config is scattered** across 3+ formats/locations: `.claude.json` (`mcpServers`), `config.toml` (`mcp_servers`), `opencode.jsonc` (`mcp`). No unified view of "what tools do my agents have."
4. **Instruction-file fragmentation:** Claude uses `CLAUDE.md`; Codex and Pi use the emerging `AGENTS.md` standard.

---

## 2. Full feature catalog

Tags: **[value]** high / med / low · **[effort]** S/M/L · **[risk]** read-only vs write · agent coverage.

### A. Session viewing & navigation

- **Multi-agent session viewer** — Claude + Codex + Pi + OpenCode in one UI. [value high] [effort M]
- **Unified cross-agent timeline** — one searchable history of everything you did with any agent, grouped by project/repo (`cwd`/`gitBranch`). [value high] [effort M]
- **Conversation-tree / fork view** — sessions are trees (`parentUuid`, `isSidechain`, `last-prompt`/`leafUuid`); visualize branches, sidechains = subagent runs. [value high] [effort M]
- **Sidechain/subagent drill-down** — expand what each spawned agent did inline.
- **Full-text search across all sessions** — by content, tool, file touched, error, model, date, repo. [value high] [effort M]
- **Filter/facet** — by model, project, branch, duration, token cost, success/error, tools used.
- **Session diff** — compare two runs of the same task.
- **"Resume from here" deep links** — jump back into a specific message in the real agent.
- **Render attachments / pasted images** (`paste-cache/`).
- **Tool-call inspector** — pretty-render each tool call + result, collapsible.
- **Token/cost overlay per message** — see where context/cost went (ties to your session-report).
- **Session replay** — step through chronologically like a debugger.

### B. Memory

- **Claude memory editor** — CRUD on file-based memories + CLAUDE.md, atomic writes, watch for external changes. [risk write]
- **Codex memory viewer** — render `stage1_outputs`; read-first (auto-generated, SQLite). [value med] [risk high if write]
- **Side-by-side memory across agents** — what each agent "knows." [value high]
- **Memory audit panel** — stale, duplicate, contradictory, PII, oversized; links to your memory-curator skill. [value high]
- **Memory provenance** — which session created/used a memory (`usage_count`, `last_usage`).
- **Memory diff/history** — track changes over time (needs snapshots).
- **"Promote" flow** — turn a Codex auto-memory into a curated Claude memory (cross-agent, teases L2).

### C. Rules / instructions

- **Rules resolver** — render the active hierarchy (managed > project > user) and answer "what instructions are live for project X right now." [value high] [effort M]
- **CLAUDE.md ⇄ AGENTS.md unified view** — reconcile the two standards.
- **Lint instructions** — contradictions, dead references, oversized files, conflicting rules across layers.
- **Per-project instruction inspector** — what each repo injects.
- **Global rules unifier (one source, symlinked)** — the *user/global*-level counterpart to the resolver above. Each agent reads a **different** global entry point: Claude `~/.claude/CLAUDE.md`, Codex `~/.codex/AGENTS.md`, Pi `~/.pi/AGENTS.md`. User opts in to a single canonical file at `~/.agents/AGENTS.md`; the app **symlinks** each agent's entry point to it (a symlink keeps the agent's own filename, e.g. `~/.claude/CLAUDE.md` → `~/.agents/AGENTS.md`), so you write your global rules **once** and every agent picks them up. Builds on "CLAUDE.md ⇄ AGENTS.md unified view." [value high] [effort M] [risk write] — **not first priority** (first = port the session + memory skills).
  - **Activate / deactivate** — opt-in toggle. Deactivate restores real files from the links (fully reversible).
  - **Safe migration** — entry points may already hold content (e.g. an existing `~/.claude/CLAUDE.md`); on activate, **back up + merge** each into the canonical file before replacing with a symlink. Never silently clobber.
  - **Status / drift view** — per agent: linked-to-canonical vs has-own-file vs missing; one-click "link this agent."
  - **Portability caveat** — symlinks are clean on macOS/Linux; Windows (future Electron) needs privilege or a copy/sync fallback. L1 target: macOS/Linux.

### D. MCP servers

- **Unified MCP dashboard** — every server across Claude/Codex/OpenCode in one list. [value high] [effort M]
- **Connection/auth status** — connected vs needs-auth (`mcp-needs-auth-cache.json`), last error.
- **Test/ping a server** from the UI.
- **Per-agent availability matrix** — which agents have which MCP.
- **MCP drift detector** — server in one agent but not another.
- **Catalog/health** — args, transport (stdio/url), enabled state.

### E. Skills

- **Skills browser/editor** — all agents, edit frontmatter + body. [value high] [effort M]
- **Skill drift detector** — same skill, different versions across Claude/Codex/Pi (you have ≥3 dupes today). [value high]
- **Skill usage stats** — from `skillUsage`.
- **Enable/disable, install/uninstall** skills.
- **Skill linter** — validate frontmatter, triggers, broken refs.
- **"Sync skill across agents"** preview (read-only diff; actual sync = L2).

### F. Subagents / commands / hooks / permissions / plugins

- **Subagent browser/editor** (`agents/`).
- **Slash-command browser/editor** (`commands/`).
- **Hooks viewer** — what's configured, when it fires, last result.
- **Permissions viewer + precedence resolver** — allow/deny across managed/project/user/local. [value med]
- **Plugin & marketplace browser** — installed, available, blocklist, usage (`pluginUsage`).

### G. Checkpoints / file history

- **Checkpoint diff + restore viewer** — expose `file-history/`: every file version an agent wrote, diff against current, restore. Nobody surfaces this. [value high] [effort M]
- **Per-session file-change timeline** — what files this session touched and how.
- **Rewind explorer** — reconstruct repo state at any point in a session.

### H. Analytics & insights

- **Usage & cost dashboard** — better than the built-in `report.html`, made cross-agent (tokens, model mix, $ estimate). [value high] [effort M]
- **Activity heatmap** — from `hourCounts` / `dailyActivity`.
- **Tool & skill leaderboards** — `toolUsage`, `skillUsage`, `pluginUsage`.
- **Cost-per-project / cost-per-repo** rollups.
- **Model usage breakdown** over time (`dailyModelTokens`, `modelUsage`).
- **Session stats** — longest session, streaks, first-session date, speculation time saved.
- **"Most expensive sessions"** + drill-in.

### I. Prompt library

- **Searchable prompt history** — 3,551 prompts in `history.jsonl`. [value high] [effort S]
- **Save/tag/favorite prompts**, build reusable snippets.
- **"Prompts that worked"** — link prompt → session outcome.
- **Prompt templates** with variables.

### J. Tasks / plans / goals

- **Background tasks viewer** (`tasks/`, Codex `agent_jobs`/`agent_job_items`). [value med]
- **Plans browser** (`plans/*.md`) + plan-mode artifacts inside sessions.
- **Codex goals viewer** (`thread_goals`).
- **Job/agent graph** — Codex `thread_spawn_edges`.

### K. Safety / hygiene

- **Secret scanner at rest** — `auth.json`, `.env.local`, `config.toml`, `settings.json`. [value high] [effort M]
- **Secrets-in-transcripts scanner** — secrets that leaked into session content (extends your session-report redaction). [value high]
- **PII scanner** across sessions + memories.
- **Disk/cleanup manager** — `projects/` 295MB, `file-history/` 28MB; safe pruning honoring `cleanupPeriodDays`; "what's safe to delete." [value med]
- **Backup/restore** — `backups/` browser; restore a config to a prior state.
- **Redacted export/share** — session → HTML/markdown/link (reuse session-report renderer). [value high]

### L. Cross-agent unification (the L1 differentiator)

- **One pane across all agents** for: sessions, memory, skills, MCP, rules.
- **Drift dashboard** — skills/MCP/rules that differ across agents (read-only; reconciliation = L2 hook).
- **Import session from another agent** — precedent exists (`external_agent_session_imports.json`).
- **Agent comparison** — "how does each agent see this project."

### M. Power-user / dev-experience

- **Global command palette** — jump to any session/skill/memory/setting.
- **Live watch** — auto-refresh as agents write files (FS watcher).
- **Local API / CLI** — query your own agent history programmatically.
- **Plugin API** for the inspector itself (custom panels).
- **Themeable, keyboard-first** navigation.
- **Read-only "safe mode"** toggle to prevent accidental edits.

---

## 3. Standout non-obvious finds (worth not losing)

- **`file-history/` = rewind data nobody exposes** → checkpoint diff/restore is a unique feature.
- **Claude already ships `usage-data/report.html`** → there's proven appetite for insights; you can do better + cross-agent.
- **`history.jsonl` = 3,551 prompts** → a prompt library is almost free.
- **Codex `external_agent_session_imports.json`** → cross-agent import is already a real pattern.
- **`session-report` skill duplicated in 3 agents** → drift detector is demoable on day one.
- **Codex memory is auto-generated SQLite, not editable text** → memory editing must be agent-aware; don't assume markdown.
- **MCP scattered across 3 formats** → unified MCP view is an obvious unmet need.

---

## 4. Open questions to resolve before building

1. **Read-only-first, or edit from day one?** Editing Codex (SQLite, auto-generated) is far riskier than Claude markdown — affects architecture.
2. **Is cross-agent (drift detector, unified MCP) in L1, or reserved for L2?** Determines whether L1 is strictly a viewer.
3. **Stack:** Tauri (Rust core, good for FS/watch/crypto later) vs Electron (fastest pure-TS iteration). L2 ambitions favor Tauri.
4. **Format stability:** `.claude` / `.codex` layouts are undocumented and change without notice — how much defensive parsing / versioning to invest up front.
5. **Naming the app.**
