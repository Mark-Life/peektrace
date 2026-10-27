# `bun build --compile --aot` and `--trim` for the CLI

**Date:** 2026-10-27 · **Bun:** PR [oven-sh/bun#45631](https://github.com/oven-sh/bun/pull/45631) CI build `1.4.4-canary+5b9c5e9f5` (experimental, unmerged) · **Baseline:** Bun 1.4.4 release · **Peak Trace:** `b1f146c`

`src/build.ts` can now compile the CLI ahead of time (`--aot`) and/or leave the
parts of Bun it never calls out of the executable (`--trim`). Both are opt-in
through environment variables; the default build, and so the released
binaries, is unchanged.

```sh
PEEKTRACE_AOT=1  bun run --cwd apps/cli build:binary   # machine code at build time
PEEKTRACE_TRIM=1 bun run --cwd apps/cli build:binary   # drop unused Bun APIs
PEEKTRACE_OUT_NAME=pt-aot ...                          # side-by-side output names
```

`aot` / `trim` need a Bun that has the flags (today: `bunx bun-pr 45631`).

## Results

Linux x64, AMD EPYC 9645 (not pinned), 62 GB RAM. Median of 11 runs, variants
interleaved, one warm-up pass first. Data is a real corpus: 1,999 transcripts
(~800 MB) in `~/.claude`, `~/.codex`, pi. Peak RSS is `/usr/bin/time %M`.

Columns: **release** = Bun 1.4.4 `--compile` (what ships today) · **JIT** = the
PR's Bun with plain `--compile` (control: same runtime as the other three) ·
**aot** = `--aot` · **aot+trim** = `--aot --trim` · **trim** = `--trim` only.

### Executable size

| | release | JIT (control) | trim | aot | aot+trim |
|---|---:|---:|---:|---:|---:|
| MB | 130.1 | 161.9 | **105.4** | 120.5 | 112.5 |
| vs release | — | +24% | **−19%** | −7% | −14% |

The PR's `bun` is ~30 MB bigger than 1.4.4 (the compiler is in it), which is why
the control is larger. `--trim` alone is the smallest here; `--aot` swaps the
bundled JS for machine code, which costs size back.

### Wall time, ms (lower is better)

| command | release | JIT | trim | aot | aot+trim | aot+trim vs release |
|---|---:|---:|---:|---:|---:|---:|
| `--version` | 303 | 315 | 284 | **66** | 68 | **4.4×** |
| `--help` | 379 | 325 | 344 | 94 | **85** | 4.5× |
| `memory ls` | 402 | 400 | 397 | 112 | **108** | 3.7× |
| `serve` → first response | 350 | 329 | 317 | 74 | **70** | 5.0× |
| `sessions analyze <id>` | 438 | 422 | 467 | 150 | **131** | 3.3× |
| `sessions ls` (1,999 files) | 1,309 | 1,359 | 1,270 | 1,072 | **1,004** | −23% |
| `stats` (60 days) | 4,530 | 4,537 | 4,594 | 3,616 | **3,564** | −21% |

### Peak RSS, MB

| command | release | JIT | trim | aot | aot+trim | aot+trim vs release |
|---|---:|---:|---:|---:|---:|---:|
| `--version` | 87.8 | 90.8 | 90.5 | 67.9 | **67.4** | −23% |
| `memory ls` | 89.9 | 93.2 | 92.9 | 68.9 | **68.5** | −24% |
| `serve` (after 20 requests) | 93.1 | 96.4 | 96.2 | 71.6 | **70.8** | −24% |
| `sessions ls` | 314.2 | 348.8 | 346.9 | 305.5 | **296.8** | −6% |
| `stats` | 208.1 | 208.8 | 208.5 | 132.2 | **132.1** | −37% |

CPU time (user+sys) for `stats`: 6.83 s → 3.93 s (−42%) with `aot+trim`;
`--version` 370 ms → 50 ms.

### Reading it

- **Startup-bound commands win the most** (3.3–5×): the release binary parses
  and JIT-warms the bundle on every launch; `--aot` ships it as machine code.
  Short commands are mostly startup, so most of the CLI's commands are in this class.
- **Long commands win 20%+ and use far less memory** (`stats`: −21% wall,
  −42% CPU, −37% RSS). `sessions ls` is I/O-bound, so it gains less.
- **`--trim` alone doesn't change speed or memory** (it removes code, not work),
  but it's the biggest size win. Combined with `--aot` it's 14% smaller than the
  release binary even though the PR's runtime is 30 MB bigger.
- `aot` vs `aot+trim` speed differences are within noise.

## Compatibility checked

For every variant: `sessions ls --json`, `sessions analyze --json <id>` and
`stats --json` (minus `generatedAt`/`window`) are byte-identical to the release
binary's output; `memory ls`, `serve` (index + asset served), and `tui` (OpenTUI's
native library `dlopen`s and draws) all start. Telemetry (`bun:sqlite`) keeps
working under `--trim`: the build sees the import and keeps it.

One source change was needed: `--aot` type-checks what it compiles, and the
generated `embedded-ui.gen.ts` (`import … with { type: "file" }`) was typed as
`HTMLBundle`/CSS module by Bun's ambient declarations, so the `as Record<string,
string>` cast failed with `TS2352`. The generator now emits
`as unknown as Record<string, string>`.

## Costs and caveats

- The AOT build takes ~16 s wall / ~132 s CPU (release `--compile`: ~1.5 s).
- Experimental: no cross-compilation (each target must be built on its own
  platform, which `build:npm`'s cross-compile matrix currently relies on), no
  breakpoints, and Windows/macOS are less tested. That is why this PR keeps the
  release pipeline on the default build; switching CI to `--aot` is a follow-up
  once Bun ships it.
- The Bun used is a PR CI artifact (commit `5b9c5e9`, not Bun's latest push), not
  a release build; numbers may shift when it lands.
- Single machine, unpinned cores; the real transcripts are not committed, so
  absolute numbers for `sessions ls`/`stats` depend on corpus size.

## Reproducing

```sh
bunx bun-pr 45631
cd apps/cli
PEEKTRACE_OUT_NAME=pt-bun144 bun run src/build.ts
PEEKTRACE_OUT_NAME=pt-jit bun-45631 run src/build.ts
PEEKTRACE_OUT_NAME=pt-aot PEEKTRACE_AOT=1 bun-45631 run src/build.ts
PEEKTRACE_OUT_NAME=pt-trim PEEKTRACE_TRIM=1 bun-45631 run src/build.ts
PEEKTRACE_OUT_NAME=pt-aot-trim PEEKTRACE_AOT=1 PEEKTRACE_TRIM=1 bun-45631 run src/build.ts
python3 scripts/bench-variants.py dist/bun-linux-x64 11 results.json
```
