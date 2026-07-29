# Changelog

## Unreleased

### Added

- **`peektrace tui`** — a terminal UI that runs *alongside* the web app. Boots
  the same loopback web server as `serve` and renders an
  [OpenTUI](https://github.com/sst/opentui) React interface over the **same**
  in-process backend (one filesystem watcher, one read model), so both surfaces
  are live at once. Four sections mirror the inspector: Sessions (list +
  context-budget analysis), Memory (cross-project vault browser, read-only in
  the terminal), Capabilities (support matrix), and Settings (agent-roots
  editor). The shared HTTP surface — routing, static assets, DNS-rebinding +
  CSRF guards, port scan — was extracted to `serve-core` so `serve` and `tui`
  can't drift.

## cli-v0.2.0 — 2026-07-16

### Added

- **OpenCode session support** (#18). Reads both storage backends — the SQLite
  store (`~/.local/share/opencode/opencode.db`, WAL) and the legacy JSON tree —
  deduping by session id with the DB winning. Uses the truthful
  `data.time.created` timestamp (never the migration-stamped `time_created`
  column) and handles the grown part-type union. Thanks to **@tenequm (Misha
  Kolesnik)** for the detailed storage-format notes in #17 — the SQLite-since-v1.2.0
  layout, the dead JSON tree, and the timestamp trap — that made this a clean
  implementation instead of a reverse-engineering slog.
- **`peektrace upgrade`** self-upgrade command (#20). Resolves the newest
  `cli-v*` release (or a pinned `--version` tag), downloads the host asset +
  `SHA256SUMS`, verifies the sha256, then atomically replaces the running binary.
  `--check` reports availability and writes nothing. Windows defers to the
  PowerShell installer (a running `.exe` can't replace itself).
- **Startup update check** on `serve` (#20). Best-effort, forked with a 1.5s
  timeout and error-swallowing so it never blocks or crashes the server; results
  cache ~24h under `PEEKTRACE_DIR`.

### Changed

- Inspector drops the redundant per-section headers (#19). The top bar already
  labels the active section, so content now starts directly beneath it.

### Upgrading

From this release forward, run `peektrace upgrade`. Coming from an older build
(no `upgrade` command yet), re-run the installer:

```sh
curl -fsSL https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.ps1 | iex
```
