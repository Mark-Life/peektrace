# Rust parity scripts

`parity.ts` runs the TS CLI and the Rust CLI side by side and diffs their output: on the fixtures, on generated edge trees and on your real corpora. It is the full check, and it is slow (about 50 min on a CI runner), because every TS case starts Bun.

CI does not run it per push. `goldens.ts` replays the TS side from `tests/goldens/linux-x64/` and runs only the Rust binary, so the fixture check takes seconds once the binary is built.

```
TS CLI ──record once──> tests/goldens/<os>-<arch>/<stage>.jsonl.gz
                              │
Rust binary ──────────> same stage code, TS results replayed ──> diffs
```

## Commands

Build the binary first. The `parity` profile is release with LTO off, so it builds fast:

```sh
bun run --filter=inspector build
cargo build --profile parity --bins --examples
```

| Command | What it does |
|---|---|
| `bun scripts/rust/goldens.ts --bin target/parity/peektrace` | The CI check: Rust against the goldens. Needs no TS CLI. |
| `bun scripts/rust/goldens.ts --write --bin target/parity/peektrace` | Records the goldens: a live parity run that also stores each TS result. Refuses to write a stage with differences. |
| `bun scripts/rust/goldens.ts --verify --bin target/parity/peektrace` | Records into a temp dir and lists the stored values that changed. |
| `bun scripts/rust/parity.ts --fixtures-only --bin target/parity/peektrace` | The full live run on fixtures, TS and Rust side by side. |
| `bun scripts/rust/parity.ts` | Fixtures plus the real corpora (`~/.claude`, `~/.codex`, `~/.pi`, OpenCode). |

Add `--stage base,serve` to `goldens.ts` to run some stages only. Stages: `base` (`sessions ls`, OpenCode, analyze/export), `search`, `memory`, `stats`, `serve`, `cli` (statusline, mcp, upgrade, help, telemetry), `tui`.

## Platforms

TS output depends on the platform, and the Rust CLI follows it: directory read order, ICU time zone aliases (`Asia/Calcutta` on macOS, `Asia/Kolkata` on Linux), and `/tmp`, which is a symlink on macOS. So goldens are per platform, in `tests/goldens/<os>-<arch>/`. Only `linux-x64`, the CI platform, is checked in.

To regenerate it in CI, use one of these. Each starts the `record goldens (<stage>)` jobs in `rust-live.yml`, which record each stage on Linux and upload a `goldens-<stage>` artifact.

- On a PR to `rust` or `rust-*`: add the `record-goldens` label. It records once per labelling; to record again, remove the label and add it back.
- On `rust`: push a commit whose message contains `[record goldens]`.
- Run `rust-live.yml` by hand. This works only once the file is on the default branch.

Then copy the artifacts in and commit them:

```sh
gh run download <run-id> --pattern 'goldens-*' --dir .scratch/goldens
cp .scratch/goldens/goldens-*/* tests/goldens/linux-x64/
```

You can also record on a Linux x64 host. Put Bun 1.3.14 first on `PATH`, keep the default root `/dev/shm/peektrace-goldens`, then run `bun scripts/rust/goldens.ts --write --bin target/parity/peektrace`. tmpfs lists a directory in creation order on some kernels and newest first on others. `<stage>.json` stores the order (`dirOrder`), and the check refuses a host whose order differs. So a set recorded on a host whose order differs from the CI runner fails in CI. Use CI when in doubt.

On macOS, `--write` records `tests/goldens/darwin-arm64/` for local checks. Git ignores it.

## When to regenerate

Re-record a stage after a change to one of its inputs. `goldens-inputs.ts` lists them:

- every stage: the TS CLI and the packages it imports (`apps/cli/src`, `packages/*/src` except `ui` and `viz`), the fixtures (`packages/core/test/fixtures`), `bun.lock`, `package.json`, and `parity.ts` with the scripts it shares
- `serve` only: `apps/inspector` and `packages/ui`, `packages/viz`
- each stage: the stage scripts it imports, for example `parity-tui.ts` for `tui` only

Each stage's `<stage>.json` keeps a hash of its inputs. The check fails with `goldens are stale` and names the stages when the hash does not match, so a change cannot skip the regeneration. Re-record only those stages: label the PR, then copy just their artifacts. Rust changes never need it.

Record with the Bun version that `package.json` pins (`packageManager`); `--write` refuses another one.

## How replay works

`parity-oracle.ts` sits under each TS call of the stage code: CLI runs, server starts, `/rpc` and raw HTTP calls, files TS wrote. The case lists, envs, fixture builders and comparisons stay in the stage modules, so live runs and replays share them.

- Each stage runs in its own process under `/dev/shm/peektrace-goldens/<stage>` on Linux, `/tmp/peektrace-goldens/<stage>` elsewhere (`PARITY_GOLDENS_ROOT` moves it). Record and check must use the same path: fixture text holds HOME paths, and their length changes `--pretty` column widths. `<stage>.json` stores the path, and the check fails when it differs. Linux uses tmpfs because ext4 lists a directory in an order that differs between machines, and the edge trees list symlinked duplicates in that order.
- The root is shared by every checkout on the host, so one run at a time holds it (`<root>/.lock`). A second run fails fast. The root must be a real directory you own, not a symlink.
- Stored text has the stage dir, its real path and the repo replaced by `<WORK>`, `<RWORK>` and `<REPO>`. The fake upgrade server, the host asset name and a closed port are bound the same way. `--write` fails if your home dir, temp dir or repo path would land in a golden.
- Fixture copies get fixed mtimes, since some rows print a file's mtime.
- Clock values TS prints (stats `generatedAt` and window, export stamps, span durations, Effect log times) are masked when recorded, keeping the shape the compare-time masks expect.
- `<stage>.index` lists each key with a short hash, so a regeneration shows as a readable diff.

## Live only

- Telemetry cross-reads: TS `doctor` on the Rust store and the reverse.
- The real-corpus suites.
- TS state Rust reads in a live run (the stats `rule-lines.json` both sides share). In replay Rust writes its own.

The nightly `parity live (TS vs Rust)` job runs `parity.ts --fixtures-only` and `goldens.ts --verify` on Linux. It is in `rust-live.yml`, which needs to be on the default branch for `schedule` and manual runs. Until then, label a PR `parity-live`: the job runs when the label is added and on each push after that.
