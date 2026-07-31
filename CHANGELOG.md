# Changelog

## Unreleased

## cli-v0.4.0 — 2026-07-31

### Added

- **Delta upgrades** (#26). `peektrace upgrade` now patches the installed
  binary in place instead of re-downloading it. Each release publishes
  `<asset>.patch` — a bsdiff + zstd delta from the previous `cli-v*` release —
  which [binpatch](https://github.com/BYK/binpatch) applies as a chain, walking
  one release at a time and verifying the result against the target's sha256.
  Measured on the releases already shipped, a typical hop is 29–92 KB against a
  110 MB binary. A release that moves Bun's own runtime bytes produces a patch
  too large to be worth downloading (0.2.0 → 0.3.0 came to 52.8% of the gzipped
  binary), and CI drops it — those upgrades fall back to a full download, as
  does anything else that misses.

### Changed

- **Gzipped release assets** (#26). Every binary now ships a `.gz` copy, and
  the installers and `peektrace upgrade` prefer it: a full download of
  `linux-x64` drops from 116 MB to 41 MB. `SHA256SUMS` still records the
  *uncompressed* digest, so the compressed, uncompressed and patched routes all
  verify against one anchor. Releases published before this change carry no
  `.gz`, and pinning one still works — every consumer falls back to the raw
  asset.

### Upgrading

```sh
peektrace upgrade
```

This release is reached with a full download; the delta path needs the new code
on both ends, so it starts working from the next release.

## cli-v0.3.0 — 2026-07-29

### Added

- **`peektrace tui`** (#23) — a terminal UI that runs *alongside* the web app.
  Boots the same loopback web server as `serve` and renders an
  [OpenTUI](https://github.com/sst/opentui) React interface over the **same**
  in-process backend (one filesystem watcher, one read model), so both surfaces
  are live at once. Four sections mirror the inspector: Sessions (list +
  context-budget analysis), Memory (cross-project vault browser, read-only in
  the terminal), Capabilities (support matrix), and Settings (agent-roots
  editor). The shared HTTP surface — routing, static assets, DNS-rebinding +
  CSRF guards, port scan — was extracted to `serve-core` so `serve` and `tui`
  can't drift.
- **Context-growth chart in the TUI** (#24). A stacked-column view of how
  context filled up across turns, drawn with block glyphs and toggled with `g`
  from the sessions pane. Mirrors the web `GrowthTimeline` off the
  already-fetched `AnalyzedSession` — per-turn slices, dumb-zone threshold, peak
  and compaction markers — so there are no backend, RPC, or schema changes.
- **Multi-root agent config** (#22, closes #21). Each agent's base now resolves
  from its own native env var — `CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
  `XDG_DATA_HOME` — so a relocated config dir no longer yields an empty session
  list. Extra roots per agent can be declared in `~/.peektrace/settings.json`
  and are unioned with the env/default root, deduped by resolved path, so a
  personal and a work account appear in one merged list with a **Source** filter.
  A new inspector **Settings** page edits those roots without hand-editing JSON.
- **Syntax highlighting in the TUI** (#25). Transcript bodies previously
  rendered flat: OpenTUI's tree-sitter ships grammars for typescript and
  markdown but not for the JSON payloads and shell commands that dominate a
  transcript. Both are now highlighted, in the same palette as the web.

### Changed

- **Substantially smaller binaries** (#25):

  | target | 0.2.0 | 0.3.0 |
  | --- | --- | --- |
  | darwin-arm64 | 80.3 MB | 71.6 MB |
  | darwin-x64 | 85.1 MB | 77.0 MB |
  | linux-x64 | 139.6 MB | 110.8 MB |
  | win32-x64 | 132.2 MB | 105.0 MB |

  Most of it is dropping `shiki`, whose ~200 bundled grammars accounted for
  9.58 MB of the 10 MB inspector UI that gets embedded whole, and no longer
  embedding the unreachable musl copy of OpenTUI's native library in Linux
  builds. The rest comes from bumping Bun to 1.3.14, which shrank its own
  runtimes by 8.6 MB on Linux and 18.3 MB on Windows.

### Fixed

- **Cross-compilation** (#25). Releases build all four platform binaries on one
  macOS runner, but `bun build --compile` resolves the *target's* native OpenTUI
  package, which a plain `bun install` never fetches — so three of the four
  targets failed to build. The release job now installs every platform's
  optional dependencies, and a preflight check names the missing package
  instead of failing deep in the build.

### Upgrading

```sh
peektrace upgrade
```

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
