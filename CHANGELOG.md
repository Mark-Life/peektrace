# Changelog

## Unreleased

## cli-v0.7.0 — 2026-08-13

### Added

- **`peektrace stats` reads the whole corpus, not one session** (#37). The
  findings that matter live across sessions — the same failure in ten of them,
  six hours of one command over a month — and nothing read more than the open
  one. A corpus scanner now streams every transcript through the existing
  parsers, so Codex, Pi and OpenCode come free: one file parsed, folded into
  accumulators, dropped, with facts cached per file on mtime. Detectors were
  measured on 3,265 local transcripts before they shipped, and the measurements
  decided what stayed. 1,070 shell calls print a failure and still exit 0,
  against 961 that set the agent's own error flag, and 1,002 of those are piped
  into `head` or `tail`, which drops the exit code; a bare `echo ===` separator,
  which zsh expands and dies on, sits in 169 errored rows across 91 sessions.
  Failure detection reads output rather than the error flag alone. Each command
  line is charged to exactly one job with `coPresentRuns` beside it, because
  89.3% of lines are compound. Calls returned at a harness ceiling are marked,
  so a suite that was cut off does not read as a slow suite. Retry loops,
  compaction frequency, permission denials, time-to-first-edit and edit thrash
  were measured and dropped, each under about 1.5% of its own denominator.
  Three surfaces read the same numbers — `peektrace stats --days N --json`, an
  inspector route, and a TUI screen beside sessions and memory — and which rows
  qualify as findings lives in core, so no surface can drift from another.
  Redaction is on by default wherever command text, cluster labels, error text
  or user text is shown; cached shards are redacted at mint time, and the CLI
  never turns it off.
- **A session reads as a chat** (#31). The session view had one shape, a dense
  disclosure row per event, which answers where the tokens went and not what
  happened. Chat mode sits beside the table, chosen by a toggle and carried in
  the hash (`?view=chat`), so a reading is a shareable link. Prompts sit right,
  model work left, everything infrastructural centred. A call and its result
  fold into one card showing `~in → ~out`, never summed. Turn boundaries become
  a rule carrying context size and model, and attachments cluster into one line
  of chips where they were injected, six before a `+N more`.

### Changed

- **Tool calls in the table say what they are** (#31). A result row used to read
  `tool-result` and nothing more, because the transcript records only
  `tool_use_id` on a result block. Calls now pair with results by `toolUseId`,
  so a result names its tool, and rows render with AI Elements' `Tool` vendored
  into `@workspace/ui`. Badges cover only what a finished transcript can
  justify: `error`, or `unanswered` for a call whose result never arrived —
  a completed call gets none. Every replacement an edit tool records renders as
  a line diff with long unchanged stretches collapsed; the diff is pure in
  `@workspace/core/diff` and falls back to a whole-block replace past a million
  LCS cells.
- **Sorting the full history by size replaces the top-25 table** (#31). Search
  and type filters still apply, and the TUI gets the same order under `s`. The
  dumb-zone divider marks a point in time, so it renders in transcript order
  only and chat mode hides size order.

### Upgrading

```sh
peektrace upgrade
```

## cli-v0.6.0 — 2026-08-10

### Added

- **Attachment rows say what they are** (#34). Every attachment in a transcript
  badged as `attachment`, so a skill listing and a file opened in the IDE looked
  identical until you expanded them. The badge now reads the attachment's own
  type with underscores as spaces — `skill listing`, `opened file in ide` — from
  one shared rule used by both the inspector and the TUI. The transform is
  mechanical rather than a lookup table, so a type Claude starts writing
  tomorrow reads as words the day it appears; a missing or empty type still
  falls back to `attachment`. History search still matches the raw title
  (`skill_listing`), so searching the humanised text finds nothing.

### Changed

- **Inspector first paint is 30% smaller** (#35). Nothing was code-split, so
  `dist/` and the first paint were the same 332.6 KiB gzip — including a
  charting library only the memory route draws, a date picker only a popover
  opens, and a MessagePack codec the transport never uses. First paint is now
  233.4 KiB gzip (−29.8%), with the memory/capabilities/settings routes, the
  session detail pane, the calendar and the toaster split behind `lazy()`
  boundaries. 66.8 KiB is genuinely gone; the rest is deferred. Method and
  per-chunk numbers in [`docs/bundle-size.md`](docs/bundle-size.md).
- **28 unimported shadcn components deleted** (#35). They cost no JavaScript but
  `globals.css` scanned them, so each added utilities to both apps'
  stylesheets: the inspector's CSS drops 162.8 → 115.0 KiB, the web app's
  162.0 → 113.8, and eight dependencies reachable only through them go with it.
  `shadcn add <name>` brings any of them back.
- **Type donut moved off Recharts to TanStack Charts** (#30). One 140px ring
  pulled in 98.5 KiB gzip of charting library and 373 lines of wrapper for its
  single consumer; both are gone, for −59.3 KiB (−15.2%) over the whole build.
  The slices now have a visible seam between them, the one deliberate visual
  change. The other four charts stay hand-written SVG and CSS.
- **The project is MIT licensed** (#36). There was no `LICENSE`, which made a
  public repo with a clone-and-build quickstart all-rights-reserved by default.
  Root `LICENSE` plus a `license` field on all 13 workspace packages, matching
  the `MIT` that `build-npm.ts` already stamped onto every published artifact.

### Fixed

- **The live transcript stays still while an agent writes** (#33). An open
  transcript dimmed and brightened on every write and never showed new
  messages. Three causes: a refetch wrapped both panes in `animate-pulse`, the
  watch poll re-rendered the whole tree twice a second, and nothing refreshed
  the open session's analysis. Freshness now shows as a dot in the header,
  the poll lives in that leaf and reads version numbers the registry drops when
  unchanged, and the open session re-validates in place. Measured over 45s with
  a write every 4s: 23 dim cycles → 0, 0 rows appended → 24, blocked main
  thread 14.4s → 3.6s, no scroll drift either way.

### Upgrading

```sh
peektrace upgrade
```

## cli-v0.5.0 — 2026-08-06

### Added

- **Linux arm64 binary** (#29). `peektrace-linux-arm64` is now built and
  released, so Ampere and Graviton VPSes and Raspberry Pis install like any
  other platform. Before this, `install.sh` on an aarch64 box failed outright
  with `unsupported Linux architecture` — no asset existed to download.
  `peektrace upgrade` resolves the same asset.
- **Linux smoke test after publish** (#29). The publish job cross-compiles
  every target on macOS and can execute none of them, so a new `smoke-linux`
  job installs the freshly published release on `ubuntu-latest` and
  `ubuntu-24.04-arm` through the real `install.sh` — covering arch detection,
  asset naming and checksum verification — then runs `--version` and boots
  `serve` against the embedded UI. It runs after publish, so it flags a bad
  release rather than gating one; repair with `workflow_dispatch`.

### Changed

- **musl is called out, not silently broken** (#29). Both Linux binaries are
  glibc-only. On Alpine the install used to "succeed" and then fail to exec
  with an opaque error; `install.sh` now detects a musl loader and points at a
  source build instead.

### Upgrading

```sh
peektrace upgrade
```

arm64 Linux has no prior release to patch from, so its first upgrade is a full
download; deltas resume next release.

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
