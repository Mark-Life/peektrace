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

Spike numbers, macOS arm64, 702 Claude sessions (1.0 GB of main transcripts, warm page cache). TS is the compiled `peektrace` 0.12.0; Rust is the release build. Measured with `hyperfine` and `/usr/bin/time -l`.

| Item | TS | Rust |
|---|---|---|
| Binary, macOS arm64 | 71.4 MB | 0.9 MB |
| Startup (`-v`) | 497 ms, 138 MB RSS | 2 ms, 2 MB RSS |
| `sessions ls --agent claude --json --limit 0` | 2.06 s | 0.088 s |
| Peak RSS for that listing | 1.6 GB | 32 MB |

Parity: `bun scripts/rust/parity.ts` shows no differences for Claude sessions, on the fixtures and on the real corpus. It does not cover Codex, Pi or OpenCode, which have no Rust reader yet (see known differences).

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
| `--otel` `Sessions.list` `malformedLines` with `--project` | counts every Claude transcript | counts only the kept project |
| Codex, Pi and OpenCode sessions | listed | no reader yet. `--agent codex`, `pi` or `opencode` exits 1 with `not supported yet`. Without `--agent`, only Claude rows, plus a note on stderr. |
| Tie order by id (same `updatedAt`) for non-ASCII ids | ICU `localeCompare` | ASCII in CLDR root order, other chars after ASCII by code point. Session ids are ASCII in practice. |
| Two roots that name the same folder (case variant, symlink) | each scanned, so every session prints twice | the folder is scanned once |
| `TZ` with a POSIX rule in `<>` (`<+03>-3`) or a `,` | Bun falls back to the OS zone | UTC, as for other POSIX rules. Only timestamps with no offset are affected. |
| `CLAUDE_CONFIG_DIR` on Windows | split on `:`, so `C:\x` becomes `C` | split on `;`, as `path.delimiter` |

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

None yet.
