# Peektrace

A **local-first, loopback-only inspector and editor for Claude Code state** —
the memories and sessions Claude scatters under `~/.claude`, surfaced in one live
pane. Scope is L1: a single machine, your own data, never the cloud and never
fed back to a model.

`peektrace serve` boots an Effect-TS core, serves a Vite/React UI + a typed
Effect-RPC contract on `127.0.0.1`, and opens the browser. Three sections:

1. **Memory** — view / create / edit / delete Claude Code memories across **all
   projects** (cross-project explorer with per-project drill-in), plus the
   forensic surface: `MEMORY.md` budget gauge (200-line / 25 KB cliff, below-fold
   entries flagged "INVISIBLE TO CLAUDE"), type donut, browse table, index↔files
   diff, `[[wikilink]]` graph. CRUD writes back to disk **atomically** (temp +
   rename, compare-and-swap on mtime).
2. **Sessions** — browse **Claude, Codex and Pi** sessions (filter by agent) and
   open one for full context-budget forensics (parity with the `session-report`
   skill): peak context vs window, budget-at-peak partition incl. the hidden
   **thinking** band, growth timeline with dumb-zone crossing and compaction
   cliffs, loaded artifacts, searchable history. Codex reports its context window
   authoritatively; Claude subagents fold into the parent. Transcripts are
   **secret-redacted by default**.
3. **Capabilities** — a feature × agent support matrix (Claude / Codex / Pi /
   OpenCode). Click a cell to see why a capability is supported / partial /
   planned / unsupported. Session browsing is live for Claude, Codex and Pi;
   memory tooling is Claude-only, and OpenCode remains a column that shows the
   gap.

### Prerequisites

- **[Bun](https://bun.com/docs/installation)** — the runtime and package manager.
- **Node.js** `>=20.9.0` — required by some tooling in the workspace.

### Quickstart

```sh
bun install

# headline — build the UI and serve it on 127.0.0.1 (opens the browser)
bun run serve
```

`bun run serve` builds `apps/inspector` then runs `peektrace serve`. To run the
binary directly (e.g. with flags):

```sh
bun run --filter=inspector build          # emit apps/inspector/dist once
bun run apps/cli/src/index.ts serve       # in-process serve
```

### Install as a global CLI

The native installer is the only distribution channel. It downloads a prebuilt
standalone binary (macOS arm64/x64, Linux x64, Windows x64) with the inspector
embedded straight from GitHub Releases, verifies its SHA-256, and needs no Node
and no build step on the target:

```sh
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.ps1 | iex
```

Installs to `~/.local/bin` (macOS/Linux) or `%LOCALAPPDATA%\peektrace\bin`
(Windows) and prints PATH guidance if needed. Pin a version with
`PEEKTRACE_VERSION=cli-v1.2.3` (env var on unix, `$env:PEEKTRACE_VERSION` on
Windows); `PEEKTRACE_INSTALL_DIR` overrides the target directory.

Supported arches are x64 everywhere plus Apple Silicon. On anything else (e.g.
Linux arm64), build from source with `bun run --cwd apps/cli build`.

Then:

```sh
peektrace serve         # loopback inspector; open the printed URL
```

Peektrace is not on npm, and there is no desktop app download yet. Anything
claiming otherwise under the name `peektrace` is not published by this project.

On a headless server (e.g. a VPS), `peektrace serve --host 0.0.0.0 --port <p>`
binds all interfaces (**no auth — firewall it yourself**); the default is
loopback-only.

### Desktop app

`apps/desktop` wraps the same compiled binary in an Electron shell — native
window, single-instance lock, and auto-update from GitHub Releases. The shell
spawns the binary as a loopback sidecar and loads its URL.

```sh
bun run desktop:dev        # run unpackaged
bun run desktop:package    # build a macOS .dmg (unsigned)
```

CI publishes the CLI binaries on `cli-v*` tags. The desktop app is **not
distributed yet** — its workflow is manual-dispatch only until the build can be
signed and notarized. Mac builds are currently **unsigned** (Gatekeeper
right-click → Open on first launch); signing is documented in
`.docs/plan/desktop-app.md`.

### One-shot CLI

The same binary exposes scriptable commands (in-process, or `--remote <url>`
against a running server). Root-level flags are placed **before** the
subcommand and apply across the CLI: `--json` (raw JSON instead of tables),
`--pretty` (aligned tables instead of compact tab-separated), `--read-only`
(refuse mutating commands), `--remote <url>`, `--otel`, and `--no-telemetry`.
`--read-only` only changes behavior for mutating commands (e.g. `memory rm`);
`--json` only affects commands that print output.

```sh
peektrace sessions ls [--project <slug>]
peektrace sessions analyze <session-id>
peektrace memory ls [project]                    # add --json for raw output
peektrace memory show <project> <name>
peektrace --read-only memory rm <project> <name> # refused, no write performed
peektrace doctor                                 # write a local support bundle
```

`peektrace doctor` collects recent local telemetry events (see Privacy posture),
recursively redacts them, and writes a JSON bundle to `~/.peektrace` (or
`PEEKTRACE_DIR`) for you to email to support. It is a support/diagnostics
export, not a system health check — nothing is uploaded.

See [`apps/cli/README.md`](apps/cli/README.md) for every command + flag and
[`apps/inspector/README.md`](apps/inspector/README.md) for dev vs prod
transport.

### Privacy posture

- **Loopback only.** The server binds `127.0.0.1`; nothing is exposed off-box.
- **Secret redaction on by default** for any rendered/exported transcript.
- **The core reads bodies, never a model.** No transcript or memory is sent to an
  LLM.
- **Safe writes.** Atomic temp-write + rename, compare-and-swap + per-file lock,
  and a compile-time read-only filesystem layer (`--read-only`). Memory edit is
  enabled only where the capability registry says the agent supports it (Claude).
- **Local telemetry, on by default, never sent anywhere.** Each CLI invocation
  writes one wide event to a local SQLite file (`~/.peektrace/telemetry.db`, or
  `PEEKTRACE_DIR`). It stays on your machine — nothing is transmitted off-box.
  Opt out with `--no-telemetry` or `PEEKTRACE_NO_TELEMETRY=1`. `peektrace doctor`
  is the only way this data leaves your box, and only if you email the redacted
  bundle yourself.

### Packages

| Package | Role |
| --- | --- |
| `packages/core` | Effect services: agents · capabilities · sessions · memory · fs · watch |
| `packages/rpc` | Effect-RPC contract + handlers + typed client |
| `apps/cli` | `peektrace` binary: one-shot commands + `serve` + npm packaging |
| `apps/inspector` | Vite + React + Effect-Atom UI |
| `apps/desktop` | Electron shell around the compiled binary (sidecar + auto-update) |

> **Distribution.** `apps/cli` compiles to a standalone `peektrace` executable
> with the inspector **embedded** (`bun run --filter=peektrace build:binary`) — it
> serves with zero external files. It ships one way today: the native installers
> in `scripts/`, fed by GitHub Releases. The same binary is also the sidecar
> inside the (undistributed) desktop app. Full plan: `.docs/plan/desktop-app.md`.

---

# Next.js Monorepo Template

> The repo is scaffolded from a turborepo + Next.js template. `apps/web`
> (Next.js marketing) and its oRPC `packages/api` are **independent** of Peektrace
> and left untouched. The template docs below still apply to that side.

A turborepo-based monorepo template with Next.js, shadcn/ui, and strict code quality via Ultracite.

## What's Inside

- `apps/web` — Next.js application
- `packages/ui` — shared shadcn/ui component library
- `packages/typescript-config` — shared TypeScript configs

## Stack

- **Runtime**: Bun
- **Build**: Turborepo
- **Linting/Formatting**: Ultracite (Biome)
- **UI**: shadcn/ui + Tailwind CSS
- **Pre-commit**: Husky + Ultracite

## Editor Setup

Open the repo in VS Code or Cursor and accept the prompt to install the recommended extensions (`.vscode/extensions.json`):

- **Biome** — formatting + linting, set as the default formatter
- **Tailwind CSS IntelliSense** — autocomplete inside `cn` / `cva` / `tv`
- **Bun** — run and debug Bun scripts
- **Pretty TypeScript Errors** / **Error Lens** — readable, inline diagnostics

Format-on-save, import organization, and lint auto-fix run on every save via Biome. An `.editorconfig` keeps other editors consistent, and `F5` debugs the Next.js app (`.vscode/launch.json`).

## Create a New Project

Using GitHub CLI:

```bash
gh repo create my-app --template Mark-Life/netxjs-monorepo --private --clone
cd my-app
bun install
bun run upgrade
```

Or from GitHub UI: click **"Use this template"** > **"Create a new repository"**, then:

```bash
git clone https://github.com/YOUR_USERNAME/my-app.git
cd my-app
bun install
bun run upgrade
```

The `upgrade` command updates Next.js, refreshes all shadcn/ui components, updates dependencies, and runs lint fixes.

## Commands

| Command | Description |
| --- | --- |
| `bun dev` | Start all apps in dev mode (web → https://web.localhost:8443) |
| `bun run build` | Build all apps and packages |
| `bun run lint` | Lint all apps and packages |
| `bun run fix` | Auto-fix formatting and lint issues |
| `bun run check` | Check for lint/format issues |
| `bun run upgrade` | Upgrade Next.js, shadcn/ui, and all deps |

The web app runs behind [portless](https://portless.sh) at `https://web.localhost:8443` — automatic HTTPS, no port juggling. It binds the unprivileged port `8443` (via `PORTLESS_PORT` in the `dev` script) so it never needs `sudo`; the first run still adds a local certificate authority to your trust store once. Prefer a clean `https://web.localhost` with no port? Drop `PORTLESS_PORT` from the script and accept a one-time `sudo` for port 443. To bypass portless entirely, run `bun run dev:app` in `apps/web` for plain `http://localhost:3000`. Change the subdomain via the `portless` key in `apps/web/package.json`.

## Adding Components

Add shadcn/ui components to the shared `ui` package:

```bash
bunx shadcn@latest add button -c packages/ui
```

Then import from `@workspace/ui`:

```tsx
import { Button } from "@workspace/ui/components/button"
```
