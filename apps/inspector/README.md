# Peektrace Inspector (`apps/inspector`)

Vite + React 19 + Tailwind v4 UI for Peektrace — a loopback-only forensic
inspector for Claude Code memories, sessions, and the capability matrix. State
is wired with [`@effect-atom/atom-react`](https://github.com/tim-smart/effect-atom)
over the typed `@workspace/rpc` Effect-RPC contract.

## Layout

```
src/
  main.tsx                      mount + ThemeProvider + globals.css
  app.tsx                       shell + hash-routed sections
  components/
    app-shell.tsx               left nav (Memory / Sessions / Capabilities)
    section-header.tsx          shared page heading
  routes/
    capabilities-route.tsx      LIVE feature × agent matrix (capabilities.list)
    memory-route.tsx            placeholder + live project overview (memory.allVaults)
    sessions-route.tsx          placeholder + live discovery count (sessions.list)
  lib/
    client.ts                   the single AtomRpc client (PeektraceClient)
    atoms.ts                    per-procedure query atoms
    result-view.tsx             render the Result loading/success/failure union
    routes.ts                   tiny zero-dep hash router
    theme.tsx                   next-themes provider + toggle (dark-first)
    vite-env.d.ts               typed import.meta.env
```

## Data layer (Effect-Atom)

One configurable client, exposed as an `AtomRpc.Tag` in `lib/client.ts`:

```ts
export class PeektraceClient extends AtomRpc.Tag<PeektraceClient>()(
  "PeektraceClient",
  { group: PeektraceRpcs, protocol } // protocol = HTTP/NDJSON → `${BASE_URL}/rpc`
) {}
```

Each procedure becomes a `Result`-typed atom:

- `PeektraceClient.query(tag, payload)` → `Atom<Result<Success, Error>>`
- `PeektraceClient.mutation(tag)` → writable `AtomResultFn` (for the Memory CRUD
  phase)

`Result` is the loading/success/failure **discriminated union** — no `isLoading`
/ `error` flag bags. Render it with `<ResultView result={...}>{(value) => …}</ResultView>`.

The **base URL is the single knob**. It defaults to `""` (same origin) so `/rpc`
resolves against whatever host is serving the page. Override at build time with
`VITE_PEEKTRACE_BASE_URL` to point the UI at a remote `peektrace serve`.

## Transport: dev vs prod

Both modes hit the **same** path (`/rpc`); only who answers it differs.

### Dev — Vite dev server + proxy

1. Start a backend in one terminal (serves `/rpc`; built UI is irrelevant here):

   ```sh
   cd apps/cli && bun run src/index.ts serve --no-open --port 4321
   ```

2. Start the Vite dev server in another:

   ```sh
   cd apps/inspector && bun run dev
   ```

`vite.config.ts` proxies `/rpc` → `http://127.0.0.1:4321`. Override the target
with `PEEKTRACE_RPC_TARGET`. HMR + the live RPC backend run side by side.

### Prod — same-origin, served by `peektrace serve`

```sh
cd apps/inspector && bun run build      # emits dist/
cd ../cli && bun run src/index.ts serve # hosts dist/ + /rpc on 127.0.0.1:4321
```

From the repo root, `bun run serve` does both in one step (build inspector → run
`peektrace serve`).

`peektrace serve` serves the static `dist/` at `/` (SPA fallback to
`index.html`) and the RPC at `/rpc` on the **same origin**, so the default
`BASE_URL=""` just works. No proxy, no CORS.

## Safety: pointing at a throwaway projects root

Memory/session resolution reads `~/.claude/projects` by default. Set
`PEEKTRACE_CLAUDE_PROJECTS` to redirect it at a temp dir (used by automated tests
so they never touch real memories):

```sh
PEEKTRACE_CLAUDE_PROJECTS=/tmp/seed-projects \
  bun run apps/cli/src/index.ts serve --no-open --port 4321
```
