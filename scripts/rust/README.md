# Rust helper scripts

| Script | What it does |
|---|---|
| `build-rune-web.sh` | Builds the experimental Rune web app into `crates/app/dist` (wasm, JS, brotli twins). `serve --ui rune` serves it when the CLI is built with `--features rune-ui`. Needs `brotli` and Python 3.11+. |
| `web-build.sh` | A verbatim copy of Rune's own web build script at the pinned rev. `build-rune-web.sh` calls it. Replace it when the Rune pin moves; do not edit it. |
| `rune-lock-align.ts` | Pins `Cargo.lock` to the versions Rune's own lock tested, for crates only the Rune app uses. Run `bun scripts/rust/rune-lock-align.ts` after a Rune bump. |
| `tui-media/run.sh` | Runs a `peektrace` binary's TUI on a synthetic HOME in a clean env, for screenshots and tapes. |
