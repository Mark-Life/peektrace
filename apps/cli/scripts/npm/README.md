# peektrace

Local, loopback-only inspector for Claude Code memories & sessions. One
self-contained binary: the headline `serve` command boots a browser UI, plus
one-shot subcommands (`sessions ls`, `memory ls`, `doctor`, …) for scripting.

## Install

```sh
npm install -g peektrace      # or: bun install -g peektrace
peektrace serve
```

The install pulls a single prebuilt binary for your platform. Supported:
macOS (arm64, x64), Linux (x64), Windows (x64). The right binary is delivered
via an `os`/`cpu`-filtered optional dependency (`peektrace-<platform>-<arch>`),
so only your platform's binary is downloaded.

Run without installing:

```sh
npx peektrace serve           # or: bunx peektrace serve
```

## Running on a server / VPS (headless)

The Linux binary runs headless. By default `peektrace serve` binds the loopback
interface (`127.0.0.1`) only — nothing is exposed off-box, and there is no
authentication. To reach it from your laptop, prefer an SSH tunnel:

```sh
# on your machine — forward local 4321 to the server's loopback
ssh -N -L 4321:127.0.0.1:4321 user@your-server
# then open http://127.0.0.1:4321
```

To expose it directly on the network instead, pass `--host`:

```sh
peektrace serve --host 0.0.0.0
```

Caution: `--host 0.0.0.0` binds all interfaces and peektrace has **no auth**.
Anyone who can reach the port gets full read (and, unless you also pass
`--read-only`, write) access to your Claude Code data. Only do this behind a
firewall / private network you trust. The default stays loopback-only.

## Troubleshooting

- `peektrace doctor` — environment / data-location checks.
- `PEEKTRACE_BIN_PATH=/path/to/peektrace peektrace …` — run an explicit binary.
- "could not locate a platform binary" — your platform has no prebuilt binary;
  reinstall, or build from source at the repo below.

## Source

https://github.com/Mark-Life/peektrace
