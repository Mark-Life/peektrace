# Rust rewrite plan

Goal: move Peektrace from TS/Bun to Rust, so it is faster, lighter and uses less memory. End state: all product code is Rust. `ratatui` runs the TUI, and Rune runs the native app and the browser UI. The only TS left is `apps/web`, an unused landing page that is out of scope here.

Work happens on a long-lived `rust` branch until the Rust version reaches parity.

## Baseline (TS today)

| Item | Value |
|---|---|
| CLI binary, linux-x64 | 102.9 MB raw, 38.1 MB gzip (`docs/binary-size.md`) |
| `packages/core` | ~41k lines incl. tests |
| `apps/cli` | ~19k lines (16 commands, MCP, OpenTUI TUI) |
| `apps/inspector` | ~21k lines React, 22-method RPC contract (`packages/rpc/src/contract.ts`) |
| `apps/desktop` | Electrobun webview around the inspector |

Spike numbers: Bun binary and Rust release binary, both built from the same commit. Command: `sessions ls --agent claude --json --limit 0`, warm page cache, median of 10 runs (`hyperfine -N`). Peak RSS is the median of 5 interleaved runs. Mac: M1 Max, 702 sessions, 1.0 GB. Linux: rs8000 (EPYC 9645), 424 sessions, 1.3 GB dir.

| Item | Bun, Mac | Rust, Mac | Bun, Linux | Rust, Linux |
|---|---|---|---|---|
| Binary, raw | 92.5 MiB | 1.0 MiB | 111.7 MiB | 1.2 MiB |
| Binary, gzip -9 | 40.9 MiB | 0.5 MiB | 51.3 MiB | 0.6 MiB |
| Startup (`--version`) | 129 ms | 2.1 ms | 395 ms | 2 ms |
| `sessions ls` (all rows) | 1228 ms | 83 ms | 2114 ms | 62 ms |
| Peak RSS for that listing | 1174 MiB | 30 MiB | 556 MiB | 27 MiB |

Parity: `bun scripts/rust/parity.ts` shows no differences for Claude, Codex, Pi and OpenCode sessions, on the fixtures, on generated edge trees and on the real corpus (`sessions ls`, `analyze`, `export`). It also covers every `stats` command (report, drill, prompt, usage, refresh) for each agent and for all agents at once, on the fixtures and on the real corpus. `sessions search` matches on every agent, on the fixtures, generated edge lines and the real corpus. `memory ls`, `show` and `rm` match on a generated memory tree and the real `~/.claude`; `rm` runs on twin copies and the two trees are diffed. The serve stage runs the TS and Rust servers side by side and compares every `/rpc` call with its `/api` route, the Host, Origin and token guards, static files and `--remote` (`--serve-only` runs just that stage). That includes search, every stats route, the inspector's prompt text, and memory reads and writes: writes run on twin HOME copies and the two trees are diffed. On the real corpus it reads a snapshot and writes only to copies of the `memory` dirs. The `cli` stage (`--cli-only`) covers `statusline` (stdin payloads, flags, `--otel` span names, the real corpus, and `--remote` against the serve pair), `mcp` (one process per side, replies matched by id and compared byte for byte, the real corpus), `upgrade` (a local fake release server: `--check` runs, and installs on temp targets through `PEEKTRACE_UPGRADE_TARGET`) and the owned help pages. It never calls GitHub and never runs TS `upgrade` without `--check`.

`stats` on the Mac corpus (7,182 transcripts, single runs): cold `stats refresh --force` takes 6.0 s in Rust and 23.4 s in TS. A warm report takes 2.6 s in Rust and 2.3 to 9.7 s in TS.

## Target layout

```
crates/
  core      parsers (claude, codex, pi, opencode), analyze, stats, memory, pricing, search, watch
  protocol  request/response types (serde), shared by server, tui, app
  server    axum HTTP API, serves embedded web UI
  cli       clap commands, MCP stdio server
  tui       ratatui, calls core in-process
  app       Rune: native window + web build served by `server`
```

The TUI and the native app call `core` directly, with no HTTP in between. Only the browser UI goes through `server`.

## Phases

0. **Spike.** Port the Claude JSONL parser and `sessions.list`. Compare binary size, startup, RSS and latency with the Bun build, and go or no-go on the numbers.
1. **Core + parity harness.** Port `packages/core` one agent parser at a time, then analyze, stats, memory, search and pricing. The harness runs TS and Rust on the same fixtures and real sessions and diffs the JSON. It runs in CI on every change.
2. **Protocol + server.** Design a new API. It does not have to match the Effect RPC wire format, so make it cleaner and lighter where possible (streaming, paging, smaller payloads). Generate TS types from Rust (`ts-rs` or `specta`) and move the TS inspector onto them, so `serve` works before Rune is ready. Embed the assets with `rust-embed`.
3. **CLI.** Port the commands: sessions, stats, memory, statusline, upgrade, export, search. Port the MCP stdio server. Use `notify` for file watching and `syntect` or tree-sitter for highlighting.
4. **TUI.** Use `ratatui`. The current OpenTUI screens are the spec.
5. **Distribution.** Use `cargo-dist` for GitHub Releases and the shell/PowerShell installers. Add musl static Linux builds (x64, arm64), which fixes the Alpine gap. `peektrace upgrade` stays. Choose the channels (npm, Homebrew, both) once everything is built, because `cargo-dist` makes that a config change. Cut over: merge `rust` into `main` and delete the TS cli, core, rpc and desktop packages.
6. **Rune app.** One Rust UI with a native window (macOS, Windows, Linux) and a web build that `serve` hosts. It replaces `apps/inspector`, `packages/ui`, `packages/viz` and `apps/desktop`. Pin a Rune `dev` rev.
7. **Telemetry sinks.** Port the SQLite wide-event store (`rusqlite`) and `peektrace doctor`, both as `tracing-subscriber` layers. Optionally add OTLP export. No module code changes, because the spans already exist.

## Telemetry rule (all phases)

Telemetry is local only today. `--otel` prints spans to stderr, and a SQLite store feeds `peektrace doctor` support bundles. Instrument each module with `tracing` spans and wide-event fields while porting it, using the TS span names and attributes as the spec. Adding them afterwards means revisiting every module. The `--otel` stderr layer lands in the spike. Only the sinks wait for phase 7.

## Known differences from the TS CLI

These are deliberate or accepted. The parity harness does not cover them.

| Area | TS | Rust |
|---|---|---|
| `settings.json` with text but no usable `roots` | `[time] WARN (#N): peektrace: ignoring unparseable settings at <path>` on stdout, which breaks `--json` | `WARN peektrace: ignoring unparseable settings at <path>` on stderr. Both hide it with `--log-level error`, `fatal` or `none`. |
| Global flags after the subcommand | some positions fail, e.g. `sessions ls --pretty --limit 1` and `sessions --json ls` | accepted in any position |
| Flag parse errors (missing value, extra positional, repeated flag) | `Received unknown argument: '<x>'` | clap's message and usage. Exit code 1 in both. |
| Non-ISO timestamps (`2026/01/07 10:00:00`, `2026-01-31 10:00:00`, `Jan 31 2026`) | `Date.parse` reads them as local time | not parsed: the row sorts last and `--since` drops it. Claude Code writes ISO with `Z`. |
| `--pretty` widths | `Bun.stringWidth` | same per char and for Lam-Alef. A few sequences still differ, e.g. a keycap with no VS16 (`#⃣`) or a prepend mark before an emoji. |
| `--otel` `Sessions.list` `malformedLines` and root `cli` `sessions.unreadable` with `--project` | count every Claude transcript | count only the kept project. The stderr note on unreadable sessions matches. |
| A `null` line in a Claude, Codex or Pi transcript, or Claude assistant `content` that is a number, boolean or object | `analyze` / `export` fail with an Effect error on stdout | the `null` line is skipped and keeps its row index; the content yields no events |
| A symlink loop under the Codex sessions root | each pass round the loop lists the same rollouts again under a longer path, until the OS fails the path | a dir already on the walk's path is not entered again, so each rollout is listed once per real path |
| Two Claude, Codex or Pi roots that name the same folder (case variant, symlink) | each scanned, so every session prints twice | the folder is scanned once. OpenCode roots follow TS. |
| `TZ` with a POSIX rule in `<>` (`<+03>-3`) or a `,` | Bun falls back to the OS zone | UTC, as for other POSIX rules. Only timestamps with no offset are affected. |
| `stats` fact cache | `$PEEKTRACE_DIR/stats/` | `$PEEKTRACE_DIR/stats-rs/`, same layout and extractor version 13. Both read and write the one `rule-lines.json` beside it. |
| `stats` on a Claude, Codex or Pi transcript with a `null` line | the whole command fails: `Could not scan the corpus: TypeError: ...` | the line is skipped |
| `stats --days` so large the window start is not a valid date | Effect `ERROR (#n)` dump on stdout | `--days N reaches past the earliest date` on stderr, exit 1 |
| `stats`, `sessions search` and `memory` with `--otel` | also print `RpcServer.*` and `RpcClient.*` spans | no RPC spans |
| `sessions search` `scannedSessions` and `unreadable` when the result is `truncated` | vary run to run: up to 8 loads are in flight when the scan stops | each thread (at most 16) loads the next session, newest first, with no batch wait; both counts cover the sessions up to the one that made `limit + 1` matches, so they do not vary |
| `sessions search` query words that look like flags | `-v` prints the version; `true` or `false` after `--json` or `--no-redact` is read as the flag's value | `-v` is a clap error; `true` and `false` are query text. Other dash words follow TS: query text, and when the query starts with one, every later word is text too. |
| `sessions search` root span `argv` | dash query words (`--force`, `-rf`) stay verbatim | the reordered command line: known flags first, then `--`, then every query word as `<text>` |
| `memory` on Windows | every entry reads as not in `MEMORY.md`: the match slugs the whole path, which has `\` separators | the path's `\` read as `/`, so entries match by file name |
| `memory` `typeCounts` for a `type` named `__proto__` or after an `Object.prototype` member (`constructor`, `toString`, ...) | `__proto__` is dropped; the others print as text such as `"function Object() { [native code] }1"` | counted like any other type |
| `memory rm` when the file cannot be removed (IO error) | Effect `ERROR (#n)` dump on stdout | `<path>: <OS error>` on stderr, naming the memory file or `MEMORY.md`. Exit 1 in both. |
| `stats usage --tz` | ICU `Intl` names and aliases | jiff names (any case) and fixed offsets `±HH`, `±HHMM`, `±HH:MM`. Rare ICU-only aliases may be rejected. |
| `stats` nested transcript walk | follows symlinked dirs with no loop guard | stops 64 dirs deep |
| `stats` rule check for a missing binary whose name is about 50,000+ chars | the name gets a probe | the probe pattern passes the regex size limit, so that name gets no probe |
| `CLAUDE_CONFIG_DIR` on Windows | split on `:`, so `C:\x` becomes `C` | split on `;`, as `path.delimiter` |
| OpenCode store locked by a writer (rollback-journal DB under `BEGIN EXCLUSIVE`) | that DB is skipped at once (busy timeout 0) | waits up to 1 s for the lock, then skips the DB. WAL stores, which OpenCode uses, read the last commit in both. |
| JSON nested deeper than 512 arrays/objects (`JsValue::parse`: OpenCode `data`, tree files, transcript records read as values, `settings.json`) | parsed; `JSON.stringify` overflows its stack only far deeper | the text reads as unparseable, as if malformed |
| OpenCode time above 8.64e15 ms | `ls` and `analyze` fail with an Effect `RangeError` on stdout | the time reads as no date: `startedAt`, `ts` and `endedAt` are left out, and `updatedAt` prints `1970-01-01T00:00:00.000Z` |
| Server transport | Effect RPC NDJSON `POST /rpc` | JSON REST under `/api`, SSE `/api/events` |
| Live updates | client polls `watch.poll` every 1 s | SSE push, one stream shared by all tabs of the origin (Web Lock leader, `BroadcastChannel` relay); `/api/watch` poll as fallback |
| Watched roots | Claude roots only | every agent's roots (OpenCode: `opencode*.db[-wal]`, `storage/{session,message,part}/**.json`) |
| Unknown GET under `/api` | `index.html` 200 | JSON 404 `NotFoundError` |
| `index.html` cache | `no-store` only when embedded | `no-store` for every source |
| Bad request payload (`agent=bogus`, settings body without `settings`) | Effect `Die` | 400 `BadRequestError` |
| Session image | `{data, mediaType}` base64, or `null` | raw bytes, or 404 `ImageNotFoundError` |
| `serve` startup lines | `RPC: <url>/rpc`, `via watch.poll` | `API: <url>/api`, `via /api/events` |
| `serve --port` near 65535, bind error text | Node messages | `No free port in range ...`, OS message |
| `serve --port abc` | `@effect/cli` integer error | clap's message. Exit code 1 in both. |
| `--remote` lenient URLs (`localhost:4630`, `http:/host`) | Bun `fetch` accepts some | parsed with WHATWG rules; no scheme gives the "Include the scheme" error |
| `--remote` with a base path (`http://h/base`) | `failed (HTTP 404)` | `failed (unexpected response)`: the server answers `index.html` for `/base/api/...` |
| `--remote --json` key order | Schema order (`provider` first) | local CLI order |
| `--remote https://` | supported | needs the `https` cargo feature (rustls); the default build has no TLS client and says so |
| `SettingsWriteError.reason` on IO failure | `String(PlatformError)` | OS error text; the temp file is removed |
| `/api` JSON over 1 KiB | sent as is | gzip (level 1) when the client accepts it |
| Bad search, stats or memory payload (missing field, bad enum, search `limit` -1 or 1.5, blank `q`, `since` `7d`, usage `days` 0, drill table `bogus`) | Effect `Die` | 400 `BadRequestError` |
| Stats `agents: []` | scans nothing | not expressible: no `agent` param means every agent |
| Stats `redact: false` | raw strings, cache skipped | 400 `BadRequestError` |
| Stats `days` past the earliest date (`1e12`) | `Die` (`RangeError: Invalid Date`) | 500 `StatsScanError` (`--days ... reaches past the earliest date`) |
| Read-only memory write | `MemoryValidationError` in the RPC exit | same body, status 403 |
| Memory vault of project `""` | an absent vault | 404 `NotFoundError` (no empty path segment) |
| Search `since` that is not ISO (`June 1, 2026`) | accepted by the legacy `Date.parse` | 400 |
| Search counts when `truncated` | `scannedSessions` and `unreadable` vary run to run | stable |
| Session id `search` | reachable through `sessions.get` | `/api/sessions/search` is the search route |
| Memory write IO failure | `Die` | 500 `text/plain` defect, logged with path and OS text |
| Stats calls at once | passes run in parallel and race on the cache index | one pass at a time per server |
| `--remote` for `stats*`, `sessions search`, `memory *` | works | refused with "--remote is not supported for ... in the Rust build yet." (exit 1) |
| `statusline` deadline | `--timeout` covers stdin, lookup and remote calls, not the parse | covers the parse too: a worker thread reads and analyzes, and a run past the deadline prints `ctx --` |
| `statusline` stdin over 1 MB | the chunk that crosses 1 MB is kept, so the cut depends on how the writer chunks the pipe | read in 64 KiB chunks. Real payloads are a few KB. |
| `upgrade --version X --check` (`--version` first) | prints the CLI version: `@effect/cli` takes it as the root flag | pins `X` |
| `upgrade` delta route (binpatch) | patches a compiled Bun binary when a patch chain exists | always a full download; the line says `downloaded <size>` |
| `upgrade` failure causes | Bun text, e.g. `SyntaxError: JSON Parse error: ...`, `TypeError: ECONNREFUSED: ...`, `Error: ENOENT: ..., open '<tmp>'` | `ureq` or OS text. `Error: GitHub API responded <n>`, `Error: download responded <n>` and `Error: fetch responded <n>` match. |
| `upgrade` target | `process.execPath` | the real path of the running binary; the hidden `PEEKTRACE_UPGRADE_TARGET` overrides it (tests and parity) |
| `upgrade` and the update notice over `https` (GitHub) | supported | need the `https` cargo feature; the default build fails with "no TLS support" and the notice stays silent |
| MCP telemetry | `RpcServer.*` and `RpcClient.*` spans; `sessions_analyze` with `latest` or `cwd` opens `rpc.sessions.list`, then `rpc.sessions.analyze` | no RPC transport spans; one `rpc.sessions.analyze` span that carries the listing's `sessions.unreadable` |
| MCP tool defect | `formatCliError` of the defect | the panic message, as an `isError` result |
| `--completions <shell>` | `@effect/cli` scripts; a bad shell gives `Invalid subcommand for peektrace - ...` | `clap_complete` scripts (`sh` gets the bash one); a bad shell gives `invalid value ... [possible values: sh, bash, fish, zsh]`. Exit code 1 in both. |
| `--wizard` | `@effect/cli` interactive builder | not ported |
| Leaf `--help` pages (`sessions ls --help`, `stats --help`, ...) | `@effect/cli` pages with ANSI | clap pages |
| Bare `sessions` / `memory` | the `@effect/cli` group page | the owned group page `sessions --help` prints |
| Root `--help` rows and the bare-command banner | list `tui` and `doctor` | leave them out until those commands exist |
| `-v`, `--version` | `0.0.0-dev` and a blank line | `0.0.0-dev` |

## Risks

| Risk | Mitigation |
|---|---|
| Parser drift gives wrong token and cost numbers | parity harness in CI, real-session corpus |
| Rune is pre-release, git-only, takes no PRs | do it last, pin rev, keep the TS inspector until Rune reaches parity |
| Rune web build is heavy or limited (WebGPU, text, a11y) | spike Rune web early, run alongside phase 2 |
| Charts, graph and transcript views are big to rebuild | port view by view, using inspector screenshots as the spec |

## Side task (any time, on `main`)

Strip Next.js out of `apps/web` and leave a static HTML placeholder. A real landing page is a separate project.

## Unresolved questions

1. `stats --days` overflow: keep the Rust message, or pick one TS can share?
2. Keep `EXTRACTOR_VERSION` 13 in Rust, or start a Rust-only counter now that the cache dirs are separate?
3. Should `stats` (report) take `--force`, as `refresh` and `usage` do? TS has none.
4. A nested transcript of the default Claude source carries no `source` while its main does. Copy TS (done) or fix both? It changes rule reach only with 2+ Claude sources.
5. The real stats parity suite snapshots `~/.claude/projects` and deletes it after. It runs by default on macOS (APFS clone) and needs `--real-stats` elsewhere (full copy). Keep it out of CI?
6. `memory rm` writes an empty MEMORY.md when the project has none. Rust copies TS. Fix it in both?
7. `memory` writes (`create`, `update`) have no `expectedHash` check and no file lock in Rust: TS has both in `fs.ts` but nothing calls them. Port them with `serve` if the server needs them?
8. TS `serve`, `tui` and the desktop sidecar serve the inspector but answer only `/rpc`, so on the `rust` branch their web UI loads no data. `scripts/rust/ts-ui-guard.ts` makes publish-cli and publish-desktop fail until this is fixed. Accept until cutover, or add a thin `/api` adapter to the TS server?
9. Watch roots that appear after `serve` starts (fresh machine): keep the TS behaviour (never watched) or watch the nearest existing parent?
10. `--remote https://` adds rustls (~1 MB). It sits behind the `https` cargo feature, off by default. Turn it on for releases?
11. Port stats `redact=false` (raw strings, no cache), or keep the 400? Only the TS RPC payload has it; no CLI flag.
12. Port `--remote` for `stats*`, `sessions search` and `memory *`? The text output needs JSON readers for each result; `--json` could pass the body through.
13. Markers now skip the server stats lock and merge into the index under a short lock. Should a corpus-wide pass waiting on the lock also time out?
14. An unknown usage `tz` is 500 `StatsScanError` (the TS tag). Move it to 400?
15. `PEEKTRACE_UPGRADE_TARGET` ships in release builds, so parity runs on the release binary. Keep it, or gate it behind a debug build and run parity on a debug binary?
16. Turn on the `https` feature (rustls, ~1 MB) by default? Without it `upgrade` and the update notice cannot reach GitHub. Links to question 10.
17. Windows `upgrade`: keep the "re-run the installer" message, or rename the running `.exe` aside and replace it?
18. Release builds need `PEEKTRACE_VERSION` at compile time, and `PEEKTRACE_RELEASE_BASE_URL`, `PEEKTRACE_RELEASE_API`, `PEEKTRACE_RELEASE_TAG_PREFIX` and `PEEKTRACE_RELEASE_ASSET_PREFIX` to point `upgrade` at Rust releases. Without all four release vars, `upgrade` refuses and the update notice stays off, unless `PEEKTRACE_BASE_URL` and `PEEKTRACE_GITHUB_API` are set at run time. Pick the tag prefix and asset names with `cargo-dist`.
19. `PEEKTRACE_DIR=""` puts `update-check.json` in the working dir, as TS does. Treat empty as unset in both?
