# Peektrace CLI (`apps/cli`)

`peektrace` — a local, **loopback-only** inspector for Claude Code memories &
sessions. One binary built on [`@effect/cli`](https://effect.website): the
headline `serve` command boots the browser UI, plus one-shot subcommands for
scripting.

All disk I/O lives in `@workspace/core`; the CLI only parses input and renders
output. The server binds `127.0.0.1` only — nothing is ever exposed off-box.

## Run it

```sh
# headline: build the UI once, then serve it (from repo root)
bun run serve                       # = build inspector + peektrace serve

# or invoke the binary directly (in-process mode)
bun run apps/cli/src/index.ts serve

# installed form (native installer, see root README)
peektrace serve
```

`./src/index.ts` is the package `bin` (`peektrace`) for local `bun run`, so a
workspace `bunx` exposes the same commands documented below.

## Distribution

The native installers (`scripts/install.sh` / `scripts/install.ps1`, at the repo
root) are the only channel. They pull the per-platform binary from the GitHub
Release that the `Publish CLI` workflow attaches to each `cli-v<semver>` tag.

`bun run --cwd apps/cli build:npm` cross-compiles every target (re-running
`src/build.ts` with `BUN_TARGET`) and stages package dirs under the gitignored
`apps/cli/dist-npm/`. That staging step is what produces the released binaries,
so it runs in CI even though nothing is published to a registry.

### npm (not published)

Publishing to npm is disabled pending name registration — the publish step in
`.github/workflows/publish-cli.yml` is commented out, and no `peektrace` package
on npm comes from this project.

The staged layout, for whenever it is re-enabled: a single unscoped `peektrace`
package shipping prebuilt binaries via `os`/`cpu`-filtered optional dependencies
— one per platform, named `peektrace-<platform>-<arch>` (`darwin-arm64`,
`darwin-x64`, `linux-x64`, `win32-x64`). Installing pulls only the host's binary;
a tiny Node shim (`peektrace.js`) resolves it and forwards argv. Publish each
`peektrace-*` variant first, then the `peektrace` wrapper (so its
optionalDependencies resolve). The scoped `@peektrace/*` naming is a documented
alternative (needs an npm org) — see the header of `scripts/build-npm.ts`.

### Running on a VPS / headless server

The Linux binary runs headless. `peektrace serve` binds loopback (`127.0.0.1`)
only by default — nothing is exposed off-box and there is **no auth**. Reach it
over an SSH tunnel:

```sh
ssh -N -L 4321:127.0.0.1:4321 user@server   # then open http://127.0.0.1:4321
```

To bind the network directly, pass `--host` (peektrace warns at startup):

```sh
peektrace serve --host 0.0.0.0                # no auth — firewall yourself
```

Only expose it behind a trusted firewall/private network; consider pairing with
`--read-only`. The default stays loopback-only.

## Execution modes

Every command runs one of two ways:

- **in-process** (default) — the CLI provisions the real `@workspace/core`
  Effect layers directly and reads `~/.claude` itself. Best for one-shot
  scripting.
- **`--remote <url>`** — the CLI becomes a thin Effect-RPC HTTP client against a
  already-running `peektrace serve` (local or a remote box over an SSH tunnel).
  No core layers are loaded locally.

## Global flags

Declared on the root command; apply to every subcommand:

| Flag | Effect |
| --- | --- |
| `--json` | Emit the raw RPC payload as JSON instead of rendered tables |
| `--read-only` | Safe mode — refuse any mutating command up-front (e.g. `memory rm`) before the write path is reached |
| `--remote <url>` | Target a running `peektrace serve` over HTTP instead of in-process |
| `--otel` | Log Effect spans to **stderr** as `[otel] <span> <ms> ok/fail {attrs}` (also enabled by the `PEEKTRACE_OTEL` env var). Off by default → no-op tracer, zero startup cost |

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
peektrace serve --no-open --port 4789
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
peektrace sessions ls
peektrace sessions ls --project -Users-me-myrepo --json
```

### `sessions analyze <id>` — context-budget forensics

Reproduces the `session-report` math headlessly: verdict
(Healthy/Degrading/Rotting), peak context vs window, final context, turn / tool
counts, the dumb-zone crossing turn, and the budget-at-peak partition.

```sh
peektrace sessions analyze <session-uuid>
peektrace sessions analyze <session-uuid> --json
```

### `memory ls [project]` — memories overview / one vault

No argument → every project that has a `memory/` directory (slug, file count,
whether a `MEMORY.md` index exists). With a project slug → that vault's entries
(name, type, in-index, size, description).

```sh
peektrace memory ls
peektrace memory ls -Users-me-myrepo
```

### `memory show <project> <name>` — print one entry

Frontmatter (name, type, description, size, modified, in-index, link count) plus
the full body.

```sh
peektrace memory show -Users-me-myrepo my-note
```

### `memory rm <project> <name>` — delete an entry

Removes the file and its `MEMORY.md` index line, then reports any now-dangling
references it left behind. **Refused** with a clear message when `--read-only`
is set — the write path is never reached.

```sh
peektrace memory rm -Users-me-myrepo my-note
peektrace --read-only memory rm -Users-me-myrepo my-note   # refused, no write
```

## Safety: point at a throwaway projects root

Resolution reads `~/.claude/projects` by default. Set `PEEKTRACE_CLAUDE_PROJECTS`
to redirect every read/write at a temp dir — used by the automated tests so they
never touch real memories:

```sh
PEEKTRACE_CLAUDE_PROJECTS=/tmp/seed-projects \
  bun run apps/cli/src/index.ts memory ls
```

## Observability

`--otel` (or `PEEKTRACE_OTEL=1`) installs a minimal console tracer at the runtime
boundary (`src/tracing.ts`, zero extra deps). Every core IO op is already
wrapped in `Effect.withSpan`, so spans print to stderr for both one-shot
commands and the long-lived `serve` fibers. To export to a real collector, swap
the console tracer for an OTLP `NodeSdk` layer behind the same flag (see the
`tracing.ts` JSDoc).
