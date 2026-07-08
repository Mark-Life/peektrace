# Peephole CLI (`apps/cli`)

`peephole` — a local, **loopback-only** inspector for Claude Code memories &
sessions. One binary built on [`@effect/cli`](https://effect.website): the
headline `serve` command boots the browser UI, plus one-shot subcommands for
scripting.

All disk I/O lives in `@workspace/core`; the CLI only parses input and renders
output. The server binds `127.0.0.1` only — nothing is ever exposed off-box.

## Run it

```sh
# headline: build the UI once, then serve it (from repo root)
bun run serve                       # = build inspector + peephole serve

# or invoke the binary directly (in-process mode)
bun run apps/cli/src/index.ts serve

# published-package form (once distributed)
bunx peephole serve
```

`./src/index.ts` is the package `bin` (`peephole`) for local `bun run`, so a
workspace `bunx` exposes the same commands documented below.

## Distribution (npm)

Published as a single unscoped package `peephole` that ships prebuilt binaries
via `os`/`cpu`-filtered optional dependencies — one per platform, named
`peephole-cli-<platform>-<arch>` (`darwin-arm64`, `darwin-x64`, `linux-x64`,
`win32-x64`). Installing pulls only the host's binary; a tiny Node shim
(`peephole.js`) resolves it and forwards argv.

```sh
npm install -g peephole      # or: bun install -g peephole
peephole serve
```

`bun run --cwd apps/cli build:npm` cross-compiles every target (re-running
`src/build.ts` with `BUN_TARGET`) and stages the publishable package dirs under
the gitignored `apps/cli/dist-npm/`. Nothing is published automatically: publish
each `peephole-cli-*` variant first, then the `peephole` wrapper (so its
optionalDependencies resolve). The scoped `@peephole/*` naming is a documented
alternative (needs an npm org) — see the header of `scripts/build-npm.ts`.

### Running on a VPS / headless server

The Linux binary runs headless. `peephole serve` binds loopback (`127.0.0.1`)
only by default — nothing is exposed off-box and there is **no auth**. Reach it
over an SSH tunnel:

```sh
ssh -N -L 4321:127.0.0.1:4321 user@server   # then open http://127.0.0.1:4321
```

To bind the network directly, pass `--host` (peephole warns at startup):

```sh
peephole serve --host 0.0.0.0                # no auth — firewall yourself
```

Only expose it behind a trusted firewall/private network; consider pairing with
`--read-only`. The default stays loopback-only.

## Execution modes

Every command runs one of two ways:

- **in-process** (default) — the CLI provisions the real `@workspace/core`
  Effect layers directly and reads `~/.claude` itself. Best for one-shot
  scripting.
- **`--remote <url>`** — the CLI becomes a thin Effect-RPC HTTP client against a
  already-running `peephole serve` (local or a remote box over an SSH tunnel).
  No core layers are loaded locally.

## Global flags

Declared on the root command; apply to every subcommand:

| Flag | Effect |
| --- | --- |
| `--json` | Emit the raw RPC payload as JSON instead of rendered tables |
| `--read-only` | Safe mode — refuse any mutating command up-front (e.g. `memory rm`) before the write path is reached |
| `--remote <url>` | Target a running `peephole serve` over HTTP instead of in-process |
| `--otel` | Log Effect spans to **stderr** as `[otel] <span> <ms> ok/fail {attrs}` (also enabled by the `PEEPHOLE_OTEL` env var). Off by default → no-op tracer, zero startup cost |

## Commands

### `serve` — the headline

Boots a loopback Bun HTTP server that mounts the Effect-RPC handler at
`POST /rpc` and serves the built inspector (`apps/inspector/dist`) at `/` with
SPA fallback to `index.html`. A scoped `WatchService` fiber watches the agent
roots for the server's lifetime so Memory + Sessions auto-refresh
(`watch.poll`).

| Flag | Default | Effect |
| --- | --- | --- |
| `--port <n>` | `4321` | Port to bind; auto-picks the next free port (up to 20) if busy |
| `--open` / `--no-open` | `--open` | Open the default browser on start; `--no-open` to skip |

```sh
peephole serve --no-open --port 4789
```

If `apps/inspector/dist` is missing, `/` returns a 503 telling you to build the
inspector first.

### `sessions ls` — list Claude sessions

Lightweight headers (lazy — bodies are not parsed). Columns: id, project, model,
message count, size, updated, title.

| Flag | Effect |
| --- | --- |
| `--agent <id>` | Agent to list. Only `claude` is wired; anything else lists empty (forward-compat) |
| `--project <slug>` | Filter sessions by project slug |

```sh
peephole sessions ls
peephole sessions ls --project -Users-me-myrepo --json
```

### `sessions analyze <id>` — context-budget forensics

Reproduces the `session-report` math headlessly: verdict
(Healthy/Degrading/Rotting), peak context vs window, final context, turn / tool
counts, the dumb-zone crossing turn, and the budget-at-peak partition.

```sh
peephole sessions analyze <session-uuid>
peephole sessions analyze <session-uuid> --json
```

### `memory ls [project]` — memories overview / one vault

No argument → every project that has a `memory/` directory (slug, file count,
whether a `MEMORY.md` index exists). With a project slug → that vault's entries
(name, type, in-index, size, description).

```sh
peephole memory ls
peephole memory ls -Users-me-myrepo
```

### `memory show <project> <name>` — print one entry

Frontmatter (name, type, description, size, modified, in-index, link count) plus
the full body.

```sh
peephole memory show -Users-me-myrepo my-note
```

### `memory rm <project> <name>` — delete an entry

Removes the file and its `MEMORY.md` index line, then reports any now-dangling
references it left behind. **Refused** with a clear message when `--read-only`
is set — the write path is never reached.

```sh
peephole memory rm -Users-me-myrepo my-note
peephole --read-only memory rm -Users-me-myrepo my-note   # refused, no write
```

## Safety: point at a throwaway projects root

Resolution reads `~/.claude/projects` by default. Set `PEEPHOLE_CLAUDE_PROJECTS`
to redirect every read/write at a temp dir — used by the automated tests so they
never touch real memories:

```sh
PEEPHOLE_CLAUDE_PROJECTS=/tmp/seed-projects \
  bun run apps/cli/src/index.ts memory ls
```

## Observability

`--otel` (or `PEEPHOLE_OTEL=1`) installs a minimal console tracer at the runtime
boundary (`src/tracing.ts`, zero extra deps). Every core IO op is already
wrapped in `Effect.withSpan`, so spans print to stderr for both one-shot
commands and the long-lived `serve` fibers. To export to a real collector, swap
the console tracer for an OTLP `NodeSdk` layer behind the same flag (see the
`tracing.ts` JSDoc).
