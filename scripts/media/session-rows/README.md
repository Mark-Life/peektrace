# Session-row media

Screenshots and layout numbers for the session list and session header in the web UI, the TUI and the Rune app. Every capture reads a synthetic HOME, never real data.

- `make-home.ts`: builds the HOME. It writes Claude, Codex and Pi transcripts, a t3code `statev2.sqlite` with fake rows, and real git worktrees of fake repos (`acme-shop`, `acme-api`). `transcripts.ts` and `t3code-db.ts` hold the builders. It needs only bun and git, and imports no project code.
- `capture.sh`: starts `peektrace serve`, the TUI (in tmux, rendered with `freeze`) and headless `peektrace-app` renders. It writes `web-*`, `tui-*` and `rune-*` PNGs plus `measure.json`.
- `measure.ts`: probes the page open in an agent-browser session. For each row it records whether content leaves the row box and the width of the chips on line 2.

```sh
bun scripts/media/session-rows/make-home.ts .scratch/media/home /tmp/acme-demo
cargo build -p peektrace-cli -p peektrace-app   # after: bun run --filter=inspector build
scripts/media/session-rows/capture.sh target/debug /tmp/acme-demo .scratch/evidence/after 4501
```

The `/tmp/acme-demo` alias is a symlink to the HOME. Transcripts, the DB and git store that path, so screenshots show it instead of this machine's paths. `WIDTHS` overrides the web viewport widths (default `1440 1024 768 600 420`). `CHIPS` overrides the CSS selector for line-2 chips (default `[data-slot=badge],[data-chip]`).
