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

Parity: `bun scripts/rust/parity.ts` shows no differences for Claude, Codex, Pi and OpenCode sessions, on the fixtures, on generated edge trees and on the real corpus (`sessions ls`, `analyze`, `export`). For Claude sessions it also covers every `stats` command (report, drill, prompt, usage, refresh), on the fixtures and on the real corpus. `stats` does not scan Codex, Pi or OpenCode yet (see known differences).

`stats` on the Mac corpus (7,172 transcripts): cold `stats refresh --force` takes 13.8 s in Rust and 23.4 s in TS. A warm report takes 4.3 s in Rust and 2.3 to 9.7 s in TS.

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
| Tie order by id (same `updatedAt`) for non-ASCII ids | ICU `localeCompare` | ASCII in CLDR root order, other chars after ASCII by code point. Session ids are ASCII in practice. |
| Two Claude, Codex or Pi roots that name the same folder (case variant, symlink) | each scanned, so every session prints twice | the folder is scanned once. OpenCode roots follow TS. |
| `TZ` with a POSIX rule in `<>` (`<+03>-3`) or a `,` | Bun falls back to the OS zone | UTC, as for other POSIX rules. Only timestamps with no offset are affected. |
| `stats` fact cache | `$PEEKTRACE_DIR/stats/` | `$PEEKTRACE_DIR/stats-rs/`, same layout and extractor version 13. Both read and write the one `rule-lines.json` beside it. |
| `stats` without `--agent`, or `--agent codex`, `pi`, `opencode` | every agent | Claude only. `--agent codex`, `pi` or `opencode` exits 1 with `not supported yet`. Without `--agent`, stderr says `<agents> sessions are not scanned: not supported yet` when those roots hold files. |
| `stats` on a transcript with a `null` line | the whole command fails: `Could not scan the corpus: TypeError: ...` | the line is skipped |
| `stats --days` so large the window start is not a valid date | Effect `ERROR (#n)` dump on stdout | `--days N reaches past the earliest date` on stderr, exit 1 |
| `stats` with `--otel` | also prints `RpcServer.stats.*` and `RpcClient.stats.*` spans | no RPC spans |
| `stats usage --tz` | ICU `Intl` names and aliases | jiff names (any case) and fixed offsets `±HH`, `±HHMM`, `±HH:MM`. Rare ICU-only aliases may be rejected. |
| `stats` nested transcript walk | follows symlinked dirs with no loop guard | stops 64 dirs deep |
| `CLAUDE_CONFIG_DIR` on Windows | split on `:`, so `C:\x` becomes `C` | split on `;`, as `path.delimiter` |
| OpenCode store locked by a writer (rollback-journal DB under `BEGIN EXCLUSIVE`) | that DB is skipped at once (busy timeout 0) | waits up to 1 s for the lock, then skips the DB. WAL stores, which OpenCode uses, read the last commit in both. |
| JSON nested deeper than 512 arrays/objects (`JsValue::parse`: OpenCode `data`, tree files, transcript records read as values) | parsed; `JSON.stringify` overflows its stack only far deeper | the text reads as unparseable, as if malformed |
| OpenCode time above 8.64e15 ms | `ls` and `analyze` fail with an Effect `RangeError` on stdout | the time reads as no date: `startedAt`, `ts` and `endedAt` are left out, and `updatedAt` prints `1970-01-01T00:00:00.000Z` |

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
5. The real-corpus parity suite copies `~/.claude/projects` (about 1 GB) per run: keep it local-only?
