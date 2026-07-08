# Peephole Desktop App + Distributable CLI — Implementation Plan

Wrap peephole (Effect Bun CLI + React/Vite inspector) into: (1) a distributable desktop app, (2) an npm-installable compiled CLI. Mirrors the `executor` open-source project.

Reference codebase (copy-adapt from): `/Users/andrey-m/Code/open-source/UsefulSoftwareCo/executor`

---

## Locked decisions

| Question | Answer |
|---|---|
| appId | `com.mark-life.peephole` · productName `Peephole` |
| Repo (public) | `Mark-Life/peephole` — updater feed owner=`Mark-Life` repo=`peephole` |
| mac signing | **No "Developer ID Application" cert on machine** (have Development + Distribution only, membership lapsed). Ship **unsigned** now; Phase F documents enabling later. |
| npm-distributable CLI | **Yes** — compiled binary installable via npm (Ubuntu VPS use case). Phase E. |
| Loopback auth token | Skip for MVP — `127.0.0.1`-only. |
| Version trigger | Tag push. CLI + desktop version **independently** (separate tags `cli-v*` / `desktop-v*`), each tracks its own `package.json`. |
| Arch coverage | Ideal: mac arm64+x64, win x64, linux x64. **Start mac-first**, broaden in Phase C.2 / H.2. |

## Architecture (target end state)

```
                 ┌─────────────────────────────────────────┐
 Electron shell  │ apps/desktop (main process)              │
 (thin wrapper)  │  - single-instance lock, window-state    │
                 │  - spawns sidecar on 127.0.0.1:<port>    │
                 │  - BrowserWindow.loadURL(sidecar url)    │
                 └───────────────┬─────────────────────────-┘
                                 │ spawn + PEEPHOLE_READY:<port> handshake
                                 ▼
                 ┌─────────────────────────────────────────┐
 sidecar =       │ compiled `peephole` binary               │
 same binary     │  (bun build --compile of apps/cli)       │
 npm also ships  │  `peephole serve --port N --client desktop`│
                 │  - BunHttpServer, one loopback origin    │
                 │  - GET /            → inspector dist (EMBEDDED in binary) │
                 │  - POST /rpc        → @effect/rpc NDJSON  │
                 │  - GET /health      → 200 ok              │
                 └─────────────────────────────────────────┘
```

Single artifact, two roles: the compiled `peephole` binary is BOTH the npm CLI and the desktop sidecar. Everything hinges on Phase A producing that binary with the inspector baked in.

## Current-state anchors (verify line numbers before editing — may drift)

- Root `package.json:17` `serve` = `bun run --filter=inspector build && bun run apps/cli/src/index.ts serve`
- `apps/cli/package.json` `bin.peephole` → `./src/index.ts` (raw TS, no compile)
- `apps/cli/src/commands/serve.ts`
  - `~:39-41` `DIST_DIR = resolve(HERE, "..","..","..","inspector","dist")` — relative to CLI **source**, breaks when packaged
  - `~:43` `--port` option already exists
  - `~:141-145` router (add `/health` here)
  - `~:150-154` human-readable port log (add machine sentinel alongside)
- `apps/inspector` — React19 + Vite + Tailwind + effect-atom, builds to `apps/inspector/dist`
- `packages/rpc` — `@effect/rpc` transport (client + server), loopback origin

---

# PHASE A — Compilable CLI + embedded inspector

**STATUS: DONE** — compiled `peephole` binary serves embedded inspector with zero external files; all validation checks pass (builds, /health ok, root HTML, PEEPHOLE_READY sentinel, runs from /tmp, RPC works). Binary builds at `apps/cli/dist/<target>/peephole`. Files: `apps/cli/src/build.ts`, `apps/cli/src/embedded-ui.gen.ts`, `apps/cli/src/commands/serve.ts`, `apps/cli/package.json`.

**Goal:** `bun build --compile apps/cli/src/index.ts` → single `peephole` binary that serves the inspector with zero external files. This is the load-bearing phase — validate in isolation before any Electron work.

**Files:**
- New `apps/cli/src/build.ts` — compile driver
- New `apps/cli/src/embedded-ui.gen.ts` — generated asset manifest (gitignored, stub committed)
- Edit `apps/cli/src/commands/serve.ts` — embedded static serving, sentinel, `/health`
- Edit `apps/cli/package.json` — build scripts

**Steps:**
1. **Embed dist.** Build script: run `vite build` on inspector → walk `apps/inspector/dist` → generate `embedded-ui.gen.ts` that imports each asset `import indexHtml from "../../inspector/dist/index.html" with { type: "file" }` and exports a `path → BunFile` map. Commit a stub `embedded-ui.gen.ts` (empty map) so type-check/dev works without a build. — done
   - Template: executor `apps/cli/src/build.ts` (its embed step) + how it re-serves embedded UI.
2. **`serve.ts` static handler — resolve order:** `PEEPHOLE_CLIENT_DIR` env (dev override) → embedded map (packaged) → relative `DIST_DIR` (source dev fallback). Fixes `:39-41`. — done
3. **`apps/cli/src/build.ts` compile command:** `bun build --compile --target=<BUN_TARGET|host> --outfile dist/<target>/peephole apps/cli/src/index.ts`. No native `.node`/WASM in peephole → simpler than executor (no sibling-file staging). — done
4. **Ready sentinel:** after server listen, if `process.env.PEEPHOLE_CLIENT === "desktop"` → `console.log("PEEPHOLE_READY:" + port)`. Keep existing human log. — done
5. **`GET /health`** in router (`~:141`) → `200 "ok"`. — done
6. **Scripts** in `apps/cli/package.json`: `"build:ui"`, `"build:embed"`, `"build:binary"` (calls `build.ts`), `"build"` = chain. — done

**Acceptance:**
- `apps/cli/dist/<host>/peephole serve --port 7777` boots, `curl 127.0.0.1:7777/health` → `ok`, `curl 127.0.0.1:7777/` → inspector HTML, inspector loads + RPC works — **run from an unrelated cwd** (proves no path deps), copy binary to `/tmp` and run there too.
- `PEEPHOLE_CLIENT=desktop peephole serve --port 7777` prints `PEEPHOLE_READY:7777`.

**Risk:** Bun `--compile` + `with { type: "file" }` embedding is the one unproven bit. Do this first, standalone.

---

# PHASE B — Electron shell (MVP desktop app)

**STATUS: PARTIAL** — `apps/desktop` (`@workspace/desktop`) implemented and `electron-vite build` passes (typechecks + installs clean). Main process has single-instance lock, `electron-window-state`, `electron-store` settings, `electron-log`, startup + sidecar-crash `data:` HTML screens, `loadURL(http://127.0.0.1:<port>/)`, `setWindowOpenHandler`→`shell.openExternal`, and an application menu. Sidecar module spawns the server on a free loopback port, parses `PEEPHOLE_READY:<port>`, probes `/health`, and does SIGTERM→(5s)→SIGKILL on quit with an expected-exit set; dev path is `bun run apps/cli/src/index.ts serve --port <p> --no-open` with `PEEPHOLE_CLIENT=desktop`. Files: `apps/desktop/src/main/index.ts`, `apps/desktop/src/main/sidecar.ts`, `apps/desktop/src/main/settings.ts`, `apps/desktop/src/main/crash-screen.ts`, `apps/desktop/src/preload/index.ts`, `apps/desktop/src/preload/global.d.ts`, `apps/desktop/src/shared/server-settings.ts`, `apps/desktop/electron.vite.config.ts`, `apps/desktop/package.json`, `apps/desktop/tsconfig.json`.
**Human-only verification still pending:** launch confirms the window renders the inspector UI (`bun run --filter=@workspace/desktop dev`); quitting the app kills the sidecar with no orphan `peephole`/`bun` process. Not yet run.

**Goal:** `apps/desktop` launches, spawns the dev sidecar, shows the inspector in a native window. Unpackaged (`electron-vite dev`) is enough for MVP.

**New package `apps/desktop`** (`@workspace/desktop`, `"type":"module"`, `"main":"./out/main/index.js"`). Copy-adapt from executor, **trimmed**:

| File | From executor | Keep | Drop for MVP |
|---|---|---|---|
| `src/main/index.ts` | `src/main/index.ts` | single-instance lock, `electron-window-state`, `electron-store` settings, `electron-log`, startup/crash `data:` HTML screens, `createWindow`→`loadURL(http://127.0.0.1:<port>/)`, `setWindowOpenHandler`→`shell.openExternal`, menu | bearer-header injection, launchd/supervised daemon, service install, MCP, Sentry |
| `src/main/sidecar.ts` | `src/main/sidecar.ts` | spawn, `PEEPHOLE_READY:<port>` parse, `/health` probe, SIGTERM→(5s)→SIGKILL on `before-quit`, expected-exit set | supervised-daemon attach, manifest |
| `src/main/settings.ts` | `src/main/settings.ts` | `electron-store`, persist `{server:{port}}` | — |
| `src/preload/index.ts` | `src/preload/index.ts` | minimal `contextBridge` (openExternal; settings later) | most channels |
| `electron.vite.config.ts` | same | main + preload targets | renderer target (use inline `data:` screens; no renderer bundle for MVP) |

**Steps:**
1. Shell picks a free port (Node `net` server → `:0` → read port, close) and passes `--port` → no scan race. — done
2. Dev sidecar cmd: `bun run apps/cli/src/index.ts serve --port <p> --no-open` with env `PEEPHOLE_CLIENT=desktop`. (Ensure `serve` has a `--no-open`/`--client` flag; add if missing.) — done (`--no-open` flag present in `serve.ts`)
3. `predev`: `bun run --filter=inspector build` so the dev sidecar finds `dist`. — done
4. Deps: `electron`, `electron-vite`, `electron-log`, `electron-store`, `electron-window-state` (dev); add `electron-updater` in Phase G. — done

**Acceptance:** `bun run --filter=@workspace/desktop dev` opens a window rendering the inspector; closing the app kills the sidecar (no orphan `peephole`/`bun` process — check `ps`). — TODO: human-only runtime verification (window renders inspector; no orphan procs on quit) not yet run.

---

# PHASE C — Packaging (electron-builder)

**STATUS: PARTIAL** — C.1 implemented; the `electron-builder.config.ts` loads cleanly and the host sidecar is staged (`apps/desktop/resources/peephole/peephole`, chmod 0o755). `build-sidecar.ts` runs the CLI's own `src/build.ts` (host target, or `BUN_TARGET` cross target) and copies the compiled binary into `resources/peephole/`. Config sets `appId: com.mark-life.peephole`, `productName: Peephole`, `artifactName: peephole-desktop-${os}-${arch}.${ext}`, `directories.output: dist-app`, `extraResources` for `resources/peephole/`, and `mac: { target: ["dmg","zip"], hardenedRuntime: true, entitlements: build/entitlements.mac.plist, notarize: false }` (unsigned). Main resolves the packaged sidecar at `process.resourcesPath/peephole/peephole`. Files: `apps/desktop/electron-builder.config.ts`, `apps/desktop/scripts/build-sidecar.ts`, `apps/desktop/build/entitlements.mac.plist`, `apps/desktop/build/icon.png`.
C.2 (win/linux) targets are present in the config (`win: nsis`, `linux: AppImage/deb/rpm`) but no non-mac artifact has been cross-built or verified.
**Human-only verification still pending:** `bun run --filter=@workspace/desktop package:mac` produces an installable `.dmg`; the installed app launches, boots the sidecar, and renders the inspector; the unsigned build passes Gatekeeper via right-click → Open on a clean macOS machine. Not yet run.

**Goal:** installable artifacts.

**Files:** `apps/desktop/electron-builder.config.ts`, `apps/desktop/scripts/build-sidecar.ts`, `apps/desktop/build/{icon.png,entitlements.mac.plist}`.

**C.1 — mac first**
1. `build-sidecar.ts` (from executor): run `apps/cli` `build.ts` for host target → copy `apps/cli/dist/<target>/peephole` → `apps/desktop/resources/peephole/` + `chmod 0o755`. — done
2. `electron-builder.config.ts`:
   - `appId: "com.mark-life.peephole"`, `productName: "Peephole"`
   - `artifactName: "peephole-desktop-${os}-${arch}.${ext}"`
   - `extraResources: [{ from: "resources/peephole/", to: "peephole/" }]` (outside asar)
   - `mac: { target: ["dmg","zip"], hardenedRuntime: true, entitlements: "build/entitlements.mac.plist", notarize: false }` (unsigned for now)
   - `directories.output: "dist-app"` — done (config loads)
3. `build/entitlements.mac.plist` ← copy executor's 4 (JIT, unsigned-executable-memory, dyld-env-vars, disable-library-validation) — required because the embedded Bun binary loads dylibs outside Electron's signing chain. — done
4. Main resolves packaged sidecar at `process.resourcesPath/peephole/peephole` (dev: `bun run …`). — done
5. `build/icon.png` — 1024×1024 RGBA peephole icon (placeholder ok initially). — done (placeholder committed)

**Acceptance:** `bun run --filter=@workspace/desktop package:mac` → `.dmg` in `dist-app/`; installed app launches, sidecar boots, inspector renders. (Unsigned → Gatekeeper right-click-open on first launch.) — TODO: human-only verification (dmg builds + installs; app launches + renders; Gatekeeper right-click-open) not yet run.

**C.2 — broaden arch** (after mac works): add `win: { target:["nsis"] }`, `linux: { target:["AppImage","deb","rpm"] }`. Do NOT pin arch in config — arch is driven per-CI-leg (Phase H). — done (targets present in config) — TODO: no win/linux artifact cross-built or verified yet.

---

# PHASE D — Dev workflow + turbo

**STATUS: DONE** — all wiring in place and typechecks. `apps/desktop/package.json` has `dev`, `build` (`bun run scripts/build-sidecar.ts && electron-vite build`), `package`/`package:mac`/`package:win`/`package:linux`, `preview`, `predev`/`prebuild` (inspector build), `typecheck`. Root `turbo.json` has `@workspace/desktop#build` depending on `inspector#build` + `peephole#build:binary` (plus a `build:binary` task). Root `package.json` keeps `serve` unchanged and adds `desktop:dev` + `desktop:package`. Files: `apps/desktop/package.json`, `turbo.json`, `package.json`.

- `apps/desktop/package.json` scripts: `dev` (`electron-vite dev`), `build` (`bun scripts/build-sidecar.ts && electron-vite build`), `package[:mac|:win|:linux]` (`electron-builder …`), `predev`/`prebuild` (inspector build). — done
- Root `turbo.json`: `@workspace/desktop#build` depends on `inspector#build` + `peephole#build:binary`. Add root scripts `desktop:dev`, `desktop:package`. — done
- Root `package.json`: keep existing `serve` (unchanged dev path); add desktop scripts. — done

---

# PHASE E — npm-distributable CLI (Ubuntu VPS use case)

**STATUS: DONE** (local staging + generation; actual `npm publish` is external, deferred to CI/Phase H). Naming resolved **unscoped** — wrapper `peephole` + per-platform `peephole-cli-<platform>-<arch>` variants (no npm org needed). Generator `scripts/build-npm.ts` cross-compiles all four targets via `BUN_TARGET` (re-running Phase A's `src/build.ts`) and stages clean publishable dirs under gitignored `dist-npm/`: main `peephole/` (Node CommonJS `bin` shim `peephole.js` + `postinstall.js` + `optionalDependencies` on all variants, `os`/`cpu` filtered) and one `peephole-cli-<platform>-<arch>/` per binary (`os`/`cpu` set so npm fetches only the host match). Shim resolves the per-platform optional dep via `require.resolve`, execs `bin/peephole[.exe]`, forwards argv/stdio/signals, propagates exit code; honors `PEEPHOLE_BIN_PATH` escape hatch. `serve.ts` gained a `--host` option (default `127.0.0.1` loopback; `--host 0.0.0.0` binds all interfaces with a no-auth WARNING; dial-host normalization for wildcard binds). Validation: pass=true — generatorRuns, stagedPackagesValid, shimExecsHostBinary, npmPackDryRun, devStillInstalls all pass; per-target builds: darwin-arm64 65.4MB, darwin-x64 70.6MB, linux-x64 104.2MB (cross-compiled from macOS), win32-x64 120.0MB (cross-compiled, `bin/peephole.exe`).

Created: `apps/cli/scripts/build-npm.ts`, `apps/cli/scripts/npm/peephole.js`, `apps/cli/scripts/npm/postinstall.js`, `apps/cli/scripts/npm/README.md`, `apps/cli/README.md`. Edited: `apps/cli/package.json` (add `build:npm` script), `apps/cli/src/commands/serve.ts` (add `--host`), `apps/cli/tsconfig.json`, `.gitignore` (ignore `dist-npm/`).

**Remaining (external):** actual `npm publish` (needs `NPM_TOKEN`, publish each `peephole-cli-*` first then `peephole`) happens in CI / Phase H — not run locally. No npm org required (unscoped).

**Goal:** `npm i -g peephole` installs the compiled binary on macOS/Linux/Windows servers.

**Pattern (executor's):** thin main package + per-platform optional-dependency binary packages.
- Main package `peephole` (from `apps/cli`): `bin` → a tiny JS shim that resolves + `execFileSync` the right `@peephole/cli-<os>-<arch>` binary from `optionalDependencies`. `postinstall` verifies presence.
- Per-platform packages `@peephole/cli-darwin-arm64`, `-darwin-x64`, `-linux-x64`, `-win32-x64`, each containing one compiled binary, with `os`/`cpu` fields so npm only fetches the matching one.
- Publish all from CI (Phase H) on `cli-v*` tag.

**Steps:**
1. Verify executor's exact layout: `apps/cli/package.json` (`bin` shim, `optionalDependencies`), its per-platform package generator script, and `scripts/build.ts` cross-compile targets. Copy-adapt. — done (adapted as `scripts/build-npm.ts` + `scripts/npm/{peephole.js,postinstall.js}`; wrapper staged to `dist-npm/` rather than mutating the private `apps/cli/package.json`)
2. Cross-compile matrix via `BUN_TARGET` (`bun-linux-x64`, `bun-darwin-arm64`, …). — done (all four targets built, incl. linux-x64 + win32-x64 cross-compiled from macOS)
3. Linux server just needs the binary + no display → `peephole serve` runs headless; user opens `http://<vps-ip>:<port>` (document binding `--host 0.0.0.0` + firewall caveat; loopback-only by default). — done (`--host` option added to `serve.ts`, default loopback, `0.0.0.0` prints no-auth WARNING)

**Note:** this reuses Phase A's binary exactly — no new compile logic, just packaging + publish.

**Open:** RESOLVED — unscoped `peephole-cli-<platform>-<arch>` (+ wrapper `peephole`); no npm org needed. Scoped `@peephole/*` was the rejected alternative.

---

# PHASE F — mac signing + notarize (DEFERRED — needs renewed membership)

Blocked: no **Developer ID Application** cert present (`security find-identity -v -p codesigning` shows only Development + Distribution). Distribution ≠ direct-download signing.

**When membership renewed:**
1. In Apple Developer portal, create a **Developer ID Application** cert; install to login keychain. Confirm: `security find-identity -v -p codesigning` lists `Developer ID Application: … (TEAMID)`.
2. Export `.p12` → CI secrets `CSC_LINK` (base64) + `CSC_KEY_PASSWORD`.
3. Notarization: App Store Connect API key → secrets `APPLE_API_KEY` (path/base64), `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`.
4. Flip `mac.notarize: true` in `electron-builder.config.ts`; `hardenedRuntime` + entitlements already set (Phase C).
5. Windows signing (optional): separate cert → `WIN_CSC_LINK`/`WIN_CSC_KEY_PASSWORD`. Skip until needed.

Until then: unsigned artifacts (right-click-open on mac; SmartScreen warning on win). Fine for early users / self-host.

---

# PHASE G — Auto-update (electron-updater + GitHub Releases)

**STATUS: DONE** — implemented across `src/shared/update.ts` (discriminated-union state + IPC channels), `src/main/updater-state.ts` (+ test), `src/main/index.ts` `setupAutoUpdater`, `electron-builder` publish block, and the `electron-updater` dependency. Checks pass: typechecks, testsPass, electronViteBuild, builderConfigLoads, publishBlockPresent (no failures).

**Files:** `apps/desktop/src/shared/update.ts`, `apps/desktop/src/main/updater-state.ts`, updater wiring in `src/main/index.ts`.

1. Copy executor `src/shared/update.ts` (discriminated-union state + IPC channels) and `src/main/updater-state.ts` (pure, unit-tested decision helpers). — done
2. `setupAutoUpdater()` (from executor `index.ts`): **packaged-only**, `electron-updater`, `provider: "github", owner: "Mark-Life", repo: "peephole"`, ~4h poll, "Restart to update" dialog → `quitAndInstall`. — done
3. `electron-builder.config.ts`: `publish: { provider: "github", owner: "Mark-Life", repo: "peephole" }` → emits `latest*.yml` manifests. — done
4. MVP UI: native dialog only (skip in-renderer update card unless a renderer bundle exists). — done

**Caveat:** unsigned auto-update works but mac Gatekeeper re-warns per update until Phase F. End-to-end update flow can only be verified against a real GitHub Release (manual — not covered by automated checks).

---

# PHASE H — CI/CD

**STATUS: DONE** (code) — all workflow + script code in place and validated. `pass=true`, checks all green: `yamlParses`, `actionlintClean`, `triggersCorrect`, `scriptsExist`, `mergeScriptTypechecks` (`referencedButMissing=[]`, `failures=[]`). Files created:
- `.github/workflows/publish-desktop.yml` — desktop release workflow.
- `.github/workflows/publish-cli.yml` — CLI npm publish workflow.
- `apps/desktop/scripts/merge-latest-mac-yml.ts` — merges dual-arch mac update manifests.
- No separate version-helper file: version is derived from the pushed tag inline in each workflow (desktop tracks `apps/desktop/package.json`, CLI tracks `apps/cli/package.json`).

**Remaining external/manual (NOT code — not done here):**
- Create GitHub repo secret `NPM_TOKEN` (required by CLI publish).
- Phase F signing/notarize secrets: `CSC_LINK`, `CSC_KEY_PASSWORD`, `APPLE_*` (for signed + notarized mac builds).
- Push the first `desktop-v*` / `cli-v*` tags to actually trigger publishing.
- End-to-end publish can only be verified by running the workflows on GitHub.

Peephole has **no Changesets** (executor does) → simpler. Independent tags per artifact.

**H.1 — desktop publish** `.github/workflows/publish-desktop.yml` (adapt executor's), trigger `push: tags: ['desktop-v*']`: — done
- Matrix start = `{macos-latest, arm64}`; expand to `{macos arm64/x64, ubuntu x64, windows x64}` in H.2. — done
- Each leg: `bun install --frozen-lockfile` → `bun run --filter=inspector build` → `bun apps/desktop/scripts/build-sidecar.ts` (with `BUN_TARGET` for the leg) → **smoke-test binary** (`peephole serve` boots + `/health` 200) → `electron-vite build` → `electron-builder --<os> --<arch> --publish never`. — done
- Release job: merge dual mac `latest-mac-<arch>.yml` (`apps/desktop/scripts/merge-latest-mac-yml.ts`), `gh release upload --clobber`, flip draft→published. Version from tag; desktop tracks `apps/desktop/package.json`. — done

**H.2 — CLI npm publish** separate workflow `.github/workflows/publish-cli.yml`, trigger `push: tags: ['cli-v*']`: — done
- Cross-compile per target → publish each `@peephole/cli-<os>-<arch>` + main `peephole` to npm (needs `NPM_TOKEN` secret — external/manual). Version from tag; tracks `apps/cli/package.json`. — done

**Why separate tags:** desktop and CLI have different cadence + distribution channels (GitHub Releases vs npm). Release either without bumping the other.

---

## Effort + sequencing

| Phase | Deliverable | Effort | Gate |
|---|---|---|---|
| **A** | compilable CLI + embedded UI + sentinel + `/health` | 1–2 d | validate standalone |
| **B** | Electron shell renders inspector (dev) — **MVP** | 2–3 d | no orphan procs |
| **C.1** | mac `.dmg` (unsigned) | 1 d | installs + runs |
| **D** | dev + turbo wiring | 0.5 d | |
| **E** | `npm i -g peephole` (VPS) | 1–2 d | reuses A binary |
| **C.2** | win/linux artifacts | 1 d | |
| **F** | mac sign+notarize | ~0.5 d work (blocked on membership) | renew first |
| **G** | auto-update | 1 d | |
| **H** | CI matrix (desktop + cli) | 2–3 d | |

**MVP = A + B + C.1 + D** (~1 week): locally-buildable, installable, unsigned mac desktop app.
**Distribution round = E + C.2 + G + H** (~1 week): npm CLI + multi-OS + updates + CI.
**F** whenever membership renews (independent).

## Template map (executor absolute paths → peephole)

```
executor/apps/cli/src/build.ts                          → apps/cli/src/build.ts (compile + embed)
executor/apps/desktop/scripts/build-sidecar.ts          → apps/desktop/scripts/build-sidecar.ts
executor/apps/desktop/electron.vite.config.ts           → apps/desktop/electron.vite.config.ts
executor/apps/desktop/electron-builder.config.ts        → apps/desktop/electron-builder.config.ts
executor/apps/desktop/src/main/index.ts                 → apps/desktop/src/main/index.ts (trim)
executor/apps/desktop/src/main/sidecar.ts               → apps/desktop/src/main/sidecar.ts (trim)
executor/apps/desktop/src/main/settings.ts              → apps/desktop/src/main/settings.ts
executor/apps/desktop/src/main/updater-state.ts         → apps/desktop/src/main/updater-state.ts (Phase G)
executor/apps/desktop/src/preload/index.ts              → apps/desktop/src/preload/index.ts (minimal)
executor/apps/desktop/src/shared/update.ts              → apps/desktop/src/shared/update.ts (Phase G)
executor/apps/desktop/build/entitlements.mac.plist      → apps/desktop/build/entitlements.mac.plist
executor/.github/workflows/publish-desktop.yml          → .github/workflows/publish-desktop.yml (Phase H)
executor/apps/desktop/scripts/merge-latest-mac-yml.ts   → apps/desktop/scripts/merge-latest-mac-yml.ts (Phase H)
executor/apps/cli/package.json (bin shim + optionalDeps)→ apps/cli npm packaging (Phase E)
```

## Secrets / env (GitHub Actions)

| Secret | Phase | Notes |
|---|---|---|
| `GITHUB_TOKEN` | G,H | auto — Releases + updater feed |
| `NPM_TOKEN` | E,H | npm publish |
| `CSC_LINK`, `CSC_KEY_PASSWORD` | F | mac Developer ID `.p12` (deferred) |
| `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` | F | notarize (deferred) |
| `WIN_CSC_LINK`, `WIN_CSC_KEY_PASSWORD` | F(win) | optional |

## Remaining implement-time decisions

- npm scope: **RESOLVED = unscoped** `peephole-cli-<os>-<arch>` (+ wrapper `peephole`); no npm org needed. `@peephole/*` scoped family rejected.
- Renderer: inline `data:` screens (MVP) vs real electron-vite renderer bundle (richer chrome, needed for in-app update UI).
- `serve` flags: confirm `--no-open` / `--client` exist; add if not.
- VPS remote access: default loopback; document `--host 0.0.0.0` + auth implications (no token yet).
